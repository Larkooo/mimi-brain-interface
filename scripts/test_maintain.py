import copy
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import maintain


def eligible_pr():
    return {
        "number": 163, "headRefOid": "candidate-head", "baseRefName": "master",
        "title": "Preserve context", "isDraft": False, "author": {"login": "owner"},
        "labels": [], "reviewDecision": "REVIEW_REQUIRED",
        "mergeable": "MERGEABLE", "additions": 100, "deletions": 10, "changedFiles": 1,
        "statusCheckRollup": [{"status": "COMPLETED", "conclusion": "SUCCESS"}],
    }


class ReleaseFixture(maintain.Controller):
    def __init__(self, root):
        super().__init__(root, root)
        self.config.update(auto_merge=True, trusted_authors=["owner"], health_window_seconds=0)
        self.config["binary"] = str(root / "installed-mimi")
        Path(self.config["binary"]).write_text("previous-binary")
        dist = root / "dashboard/dist"
        dist.mkdir(parents=True)
        (dist / "index.html").write_text("previous-dashboard")
        self.repository = "owner/mimi"
        self.actions = []
        self.fail_candidate_health = False
        self.fail_rollback = False
        self.different_merge_tree = False
        self.changed_head = False
        self.legacy_candidate = False
        self.remote_head = "merged"
        self.installed = "previous"

    def git(self, *arguments, cwd=None, timeout=60):
        if arguments[:2] == ("worktree", "add"):
            Path(arguments[3]).mkdir(parents=True)
        if arguments[0] == "ls-remote":
            return "base\trefs/heads/master"
        if arguments[:2] == ("rev-parse", "origin/master"):
            return "base"
        if arguments[:2] == ("rev-parse", "FETCH_HEAD"):
            return "candidate-head"
        if arguments[0] == "rev-parse" and arguments[1].endswith("^{tree}"):
            return "unexpected-tree" if self.different_merge_tree else "tested-tree"
        if arguments[0] == "write-tree":
            return "tested-tree"
        if "--unified=0" in arguments:
            return "+#[test]\n+fn preserves_context() {}"
        return ""

    def gh(self, *arguments):
        if arguments[:2] == ("pr", "diff"):
            return "+#[test]\n+fn preserves_context() {}"
        if arguments[:2] == ("pr", "merge"):
            self.actions.append("github-merge")
            return ""
        if arguments[-1] == "files,isCrossRepository":
            return json.dumps({"files": [{"path": "src/context_buffer.rs"}], "isCrossRepository": False})
        if arguments[-1] == "mergeCommit,state":
            return json.dumps({"mergeCommit": {"oid": "merged"}, "state": "MERGED"})
        if arguments[-1] == "state,headRefOid,mergeCommit":
            return json.dumps({"mergeCommit": {"oid": "merged"}, "state": "MERGED", "headRefOid": "candidate-head"})
        result = eligible_pr()
        if self.changed_head:
            result["headRefOid"] = "changed-head"
        return json.dumps(result)

    def api(self, method, path, payload=None):
        if method == "GET" and "/commits/" in path:
            return {"parents": [{"sha": "base"}]}
        if method == "GET":
            return {"object": {"sha": self.remote_head}}
        if method == "POST":
            self.actions.append("create-revert")
            return {"sha": "revert-commit"}
        if method == "PATCH":
            if payload["force"]:
                raise AssertionError("must never force-update GitHub")
            self.actions.append("source-revert")
            self.remote_head = payload["sha"]
            return {}

    def service_states(self):
        return {"mimi-dashboard": {"ActiveState": "active", "SubState": "running", "MainPID": "42"}}

    def healthy(self, expected, expected_binary=None):
        return self.service_states()

    def install(self, release):
        kind = "candidate" if str(release).endswith("-candidate") else "previous"
        self.actions.append("install-" + kind)
        if kind == "previous" and self.fail_rollback:
            raise maintain.MaintenanceError("disk unavailable during rollback")
        self.installed = kind

    def restart(self, services):
        self.actions.append("restart")

    def watch(self, services, binary_hash):
        self.actions.append("watch-" + self.installed)
        if self.installed == "candidate" and self.fail_candidate_health:
            raise maintain.MaintenanceError("candidate became unhealthy")

    def build_command(self, command, cwd=None, **kwargs):
        if command[0] == "claude":
            return json.dumps({"subtype": "success", "is_error": False, "result": json.dumps({
                "verdict": "approve", "reason": "Fixes lost conversation context",
                "test_evidence": "preserves_context fails before the repair", "risks": [],
            })})
        if command[-1] == "--help" and not self.legacy_candidate:
            return "--apply --review --status --force --restart"
        if command[:3] == ["cargo", "build", "--release"]:
            binary = self.directory / "build-cache/release/mimi"
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.write_text("candidate-binary")
        if command[:3] == ["bun", "run", "build"]:
            dist = Path(cwd) / "dist"
            dist.mkdir(parents=True, exist_ok=True)
            (dist / "index.html").write_text("candidate-dashboard")
        return ""


class PolicyTests(unittest.TestCase):
    def setUp(self):
        self.config = {**maintain.DEFAULTS, "auto_merge": True, "trusted_authors": ["owner"]}

    def test_eligible_requires_checks_but_not_owner_labels_or_review(self):
        self.assertIsNone(maintain.eligibility(eligible_pr(), self.config, {}))
        for key, value in [
            ("statusCheckRollup", []), ("author", {"login": "stranger"}),
            ("isDraft", True), ("reviewDecision", "CHANGES_REQUESTED"),
            ("additions", 10000), ("mergeable", "UNKNOWN"),
        ]:
            candidate = eligible_pr()
            candidate[key] = value
            with self.subTest(key=key):
                self.assertIsNotNone(maintain.eligibility(candidate, self.config, {}))

    def test_optional_label_gate_can_still_be_configured(self):
        self.config["merge_label"] = "mimi:automerge"
        self.assertIsNotNone(maintain.eligibility(eligible_pr(), self.config, {}))
        candidate = eligible_pr()
        candidate["labels"] = [{"name": "mimi:automerge"}]
        self.assertIsNone(maintain.eligibility(candidate, self.config, {}))

    def test_failed_pending_and_skipped_checks_block_release(self):
        for check in [
            {"status": "IN_PROGRESS", "conclusion": None},
            {"status": "COMPLETED", "conclusion": "FAILURE"},
            {"status": "COMPLETED", "conclusion": "SKIPPED"},
            {"state": "PENDING"}, {"state": "ERROR"},
        ]:
            candidate = eligible_pr()
            candidate["statusCheckRollup"] = [check]
            self.assertIsNotNone(maintain.eligibility(candidate, self.config, {}))

    def test_quarantine_is_for_exact_revision(self):
        candidate = eligible_pr()
        quarantine = {candidate["headRefOid"]: {"reason": "failed"}}
        self.assertIsNotNone(maintain.eligibility(candidate, self.config, quarantine))
        candidate["headRefOid"] = "fixed-head"
        self.assertIsNone(maintain.eligibility(candidate, self.config, quarantine))

    def test_protected_paths_cannot_self_modify_release_policy(self):
        self.assertIsNotNone(maintain.path_block(["scripts/maintain.py"], self.config))
        self.assertIsNotNone(maintain.path_block(["src/commands/secret.rs"], self.config))
        self.assertIsNone(maintain.path_block(["src/context_buffer.rs"], self.config))

    def test_repeated_observations_update_one_issue_and_resolve_missing_issue(self):
        finding = {"id": "task:1", "severity": "medium", "evidence": "stale"}
        first = maintain.update_observations({}, [finding], 100)
        second = maintain.update_observations(first, [finding], 200)
        self.assertEqual(len(second), 1)
        self.assertEqual(second["task:1"]["observations"], 2)
        self.assertEqual(second["task:1"]["first_seen"], 100)
        resolved = maintain.update_observations(second, [], 300)
        self.assertEqual(resolved["task:1"]["status"], "resolved")
        self.assertEqual(first["task:1"]["status"], "open")

    def test_corrupt_state_does_not_reset_safety_gates(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "state.json"
            path.write_text("{broken")
            with self.assertRaises(maintain.MaintenanceError):
                maintain.read_json(path, {})

    def test_collector_failure_does_not_resolve_previous_findings(self):
        finding = {"id": "task:1", "severity": "medium", "evidence": "stale"}
        previous = maintain.update_observations({}, [finding], 100)
        current = maintain.update_observations(previous, [], 200, ["task:"])
        self.assertEqual(current["task:1"]["status"], "open")

    def test_command_timeout_is_bounded(self):
        with self.assertRaisesRegex(maintain.MaintenanceError, "timed out"):
            maintain.run(["sleep", "5"], timeout=0.02)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.controller = ReleaseFixture(Path(self.temporary.name))
        self.runner = patch.object(maintain, "run", self.controller.build_command)
        self.runner.start()
        self.addCleanup(self.runner.stop)

    def test_healthy_candidate_is_deployed_before_github_merge_and_enters_probation(self):
        self.controller.release(eligible_pr())
        self.assertLess(self.controller.actions.index("watch-candidate"), self.controller.actions.index("github-merge"))
        self.assertIn("probation", self.controller.state)
        self.assertNotIn("pending", self.controller.state)

    def test_reviewer_failure_keeps_previous_brief_and_enforces_cooldown(self):
        brief = {"brief": "Previous evidence-backed plan"}
        maintain.atomic_json(self.controller.directory / "review.json", brief)
        snapshot = {"findings": [{"id": "backlog:pull-requests"}], "pull_requests": []}
        with patch.object(maintain, "run", return_value=json.dumps({"subtype": "error_max_budget_usd", "is_error": True})) as reviewer:
            self.controller.review(snapshot)
            self.controller.review(snapshot)
        self.assertEqual(reviewer.call_count, 1)
        self.assertEqual(maintain.read_json(self.controller.directory / "review.json", {}), brief)
        self.assertIn("review_error", self.controller.state)

    def test_diff_rejection_never_reaches_build_or_install(self):
        verdict = {"verdict": "reject", "reason": "test does not cover the bug", "test_evidence": "", "risks": ["missing test"]}
        with patch.object(maintain, "run", return_value=json.dumps({"subtype": "success", "result": json.dumps(verdict)})):
            self.controller.release(eligible_pr())
        self.assertNotIn("install-candidate", self.controller.actions)
        self.assertIn("candidate-head", self.controller.state["quarantined"])

    def test_diff_review_outage_retries_without_permanent_quarantine(self):
        with patch.object(maintain, "run", side_effect=maintain.MaintenanceError("provider unavailable")):
            self.controller.release(eligible_pr())
        self.assertNotIn("candidate-head", self.controller.state["quarantined"])
        self.assertIn("candidate-head", self.controller.state["review_retry_after"])
        self.assertIsNone(self.controller.candidate({"findings": [], "pull_requests": [eligible_pr()]}))
        self.assertNotIn("install-candidate", self.controller.actions)

    def test_diff_review_cache_is_bound_to_base_and_head(self):
        root = Path(self.temporary.name)
        self.controller.review_candidate(eligible_pr(), "base", root, "diff", [])
        self.controller.review_candidate(eligible_pr(), "base", root, "diff", [])
        self.assertEqual(self.controller.state["pr_review_budget"]["used"], 1)
        self.controller.review_candidate(eligible_pr(), "new-base", root, "diff", [])
        self.assertEqual(self.controller.state["pr_review_budget"]["used"], 2)

    def test_diff_approval_with_unresolved_risks_is_not_authorization(self):
        verdict = {"verdict": "approve", "reason": "Maybe safe", "test_evidence": "one test", "risks": ["data loss"]}
        with patch.object(maintain, "run", return_value=json.dumps({"subtype": "success", "result": json.dumps(verdict)})):
            self.controller.release(eligible_pr())
        self.assertNotIn("install-candidate", self.controller.actions)

    def test_diff_review_has_a_daily_budget(self):
        self.controller.config["max_pr_reviews_per_day"] = 0
        self.controller.release(eligible_pr())
        self.assertIn("candidate-head", self.controller.state["review_retry_after"])
        self.assertNotIn("install-candidate", self.controller.actions)

    def test_missing_ci_is_dispatched_once_without_owner_action(self):
        candidate = eligible_pr()
        candidate["statusCheckRollup"] = []
        snapshot = {"master": "base", "pull_requests": [candidate], "findings": []}
        with patch.object(self.controller, "gh", wraps=self.controller.gh) as github:
            self.controller.maintain_ci(snapshot)
            with patch.object(self.controller, "api", return_value={"workflow_runs": []}):
                self.controller.maintain_ci(snapshot)
        dispatches = [call for call in github.call_args_list if call.args[:2] == ("workflow", "run")]
        self.assertEqual(len(dispatches), 1)
        self.assertEqual(self.controller.state["ci_budget"]["used"], 1)
        self.assertIsNone(self.controller.candidate(snapshot))

    def test_only_matching_successful_ci_unblocks_candidate(self):
        candidate = eligible_pr()
        candidate["statusCheckRollup"] = []
        snapshot = {"master": "base", "pull_requests": [candidate], "findings": []}
        self.controller.maintain_ci(snapshot)
        result = {"id": 42, "head_sha": "base", "display_title": "Mimi CI candidate-head base", "status": "completed", "conclusion": "success"}
        with patch.object(self.controller, "api", return_value={"workflow_runs": [result]}):
            self.controller.maintain_ci(snapshot)
        self.assertIsNotNone(self.controller.candidate(snapshot))
        self.assertFalse(self.controller.verified_ci({**candidate, "statusCheckRollup": []}, "new-base")["statusCheckRollup"])

    def test_ci_result_identity_is_rechecked_before_deployment(self):
        candidate = eligible_pr()
        candidate["statusCheckRollup"] = []
        self.controller.state["ci_requests"] = {"candidate-head:base": {"run_id": 42, "status": "completed", "conclusion": "success"}}
        with patch.object(self.controller, "api", return_value={"head_sha": "different-base", "event": "workflow_dispatch"}):
            with self.assertRaisesRegex(maintain.MaintenanceError, "does not match"):
                self.controller.verified_ci(candidate, "base", refresh=True)

    def test_ci_dispatch_failure_is_bounded_and_retryable(self):
        candidate = eligible_pr()
        candidate["statusCheckRollup"] = []
        snapshot = {"master": "base", "pull_requests": [candidate], "findings": []}
        original = self.controller.gh
        def fail_dispatch(*arguments):
            if arguments[:2] == ("workflow", "run"):
                raise maintain.MaintenanceError("network unavailable")
            return original(*arguments)
        with patch.object(self.controller, "gh", side_effect=fail_dispatch):
            self.controller.maintain_ci(snapshot)
            self.controller.maintain_ci(snapshot)
        self.assertEqual(self.controller.state["ci_budget"]["used"], 1)
        self.assertEqual(self.controller.state["ci_requests"]["candidate-head:base"]["status"], "dispatch-failed")

    def test_bad_canary_rolls_back_without_merging_and_quarantines_revision(self):
        self.controller.fail_candidate_health = True
        self.controller.release(eligible_pr())
        self.assertNotIn("github-merge", self.controller.actions)
        self.assertEqual(self.controller.installed, "previous")
        self.assertIn("candidate-head", self.controller.state["quarantined"])
        self.assertNotIn("blocked", self.controller.state)
        self.assertIn("next_release_after", self.controller.state)

    def test_merge_of_untested_tree_triggers_rollback(self):
        self.controller.different_merge_tree = True
        self.controller.release(eligible_pr())
        self.assertIn("github-merge", self.controller.actions)
        self.assertEqual(self.controller.installed, "previous")
        self.assertIn("different tree", self.controller.state["last_rollback"]["reason"])
        self.assertIn("source-revert", self.controller.actions)

    def test_changed_pr_head_never_reaches_install(self):
        self.controller.changed_head = True
        self.controller.release(eligible_pr())
        self.assertNotIn("install-candidate", self.controller.actions)
        self.assertNotIn("github-merge", self.controller.actions)

    def test_candidate_cannot_restore_legacy_nightly_automation(self):
        self.controller.legacy_candidate = True
        self.controller.release(eligible_pr())
        self.assertNotIn("install-candidate", self.controller.actions)
        self.assertIn("predates safe maintenance", self.controller.state["quarantined"]["candidate-head"]["reason"])

    def test_health_probe_allows_bounded_startup_then_checks_stability(self):
        with patch.object(self.controller, "healthy", side_effect=[
            maintain.MaintenanceError("starting"), self.controller.service_states(), self.controller.service_states(),
        ]) as probe, patch.object(maintain.time, "sleep"):
            maintain.Controller.watch(self.controller, ["mimi-dashboard"], "hash")
        self.assertEqual(probe.call_count, 3)

    def test_health_regression_during_probation_rolls_back(self):
        self.controller.release(eligible_pr())
        self.controller.fail_candidate_health = True
        self.controller.recover()
        self.assertEqual(self.controller.installed, "previous")
        self.assertNotIn("probation", self.controller.state)

    def test_failed_rollback_preserves_journal_for_next_run(self):
        self.controller.fail_candidate_health = True
        self.controller.fail_rollback = True
        with self.assertRaisesRegex(maintain.MaintenanceError, "disk unavailable"):
            self.controller.release(eligible_pr())
        saved = maintain.read_json(self.controller.state_path, {})
        self.assertEqual(saved["pending"]["stage"], "rolling-back")
        self.controller.fail_rollback = False
        self.controller.recover()
        self.assertEqual(self.controller.installed, "previous")
        self.assertNotIn("pending", self.controller.state)

    def test_interrupted_build_does_not_restart_runtime(self):
        self.controller.state["pending"] = {"stage": "building"}
        self.controller.recover()
        self.assertEqual(self.controller.actions, [])
        self.assertNotIn("pending", self.controller.state)

    def test_interrupted_install_recovers_previous_artifact(self):
        self.controller.release(eligible_pr())
        self.controller.state["pending"] = copy.deepcopy(self.controller.state.pop("probation"))
        self.controller.state["pending"]["stage"] = "installing"
        self.controller.recover()
        self.assertEqual(self.controller.installed, "previous")
        self.assertIn("source-revert", self.controller.actions)

    def test_source_rollback_never_overwrites_newer_commits(self):
        self.controller.release(eligible_pr())
        self.controller.remote_head = "another-commit"
        self.controller.fail_candidate_health = True
        self.controller.recover()
        self.assertEqual(self.controller.installed, "previous")
        self.assertNotIn("source-revert", self.controller.actions)
        self.assertIn("blocked", self.controller.state)
        self.assertIn("source_rollback", self.controller.state)

    def test_source_revert_is_idempotent_after_a_crash(self):
        self.controller.release(eligible_pr())
        release = copy.deepcopy(self.controller.state["probation"])
        self.controller.revert_source(release)
        self.controller.revert_source(release)
        self.assertEqual(self.controller.actions.count("source-revert"), 1)

    def test_health_failure_and_probation_prevent_selection(self):
        snapshot = {"pull_requests": [eligible_pr()], "findings": []}
        self.assertEqual(self.controller.candidate(snapshot)["number"], 163)
        snapshot["findings"] = [{"id": "health:probe"}]
        self.assertIsNone(self.controller.candidate(snapshot))
        snapshot["findings"] = []
        self.controller.state["probation"] = {"until": 1000}
        self.assertIsNone(self.controller.candidate(snapshot))


if __name__ == "__main__":
    unittest.main()
