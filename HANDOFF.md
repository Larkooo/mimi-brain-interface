# Mimi Brain Interface — Full Context Handoff

You are working on `mimi-brain-interface`, an open-source framework that wraps Claude Code to create an autonomous AI assistant ("Mimi") with persistent memory, a knowledge graph, channels, and self-management capabilities.

The repo is at `~/mimi-brain-interface`. Mimi's private data lives at `~/.mimi/`.

## Current State

### Rust CLI (`src/`)
A Rust binary (`mimi`) that wraps Claude Code. Commands:
- `mimi` — launch Claude Code in tmux with `--dangerously-skip-permissions` + enabled channels
- `mimi setup` — init `~/.mimi/`, create brain.db, copy CLAUDE.md template
- `mimi status` — session info, brain stats
- `mimi dashboard` — start web dashboard (axum server on port 3131)
- `mimi brain stats/add/link/search/list/query` — knowledge graph operations
- `mimi channel add/list/remove` — channel management (installs Claude Code plugins)
- `mimi mcp add/list/remove` — wraps `claude mcp`
- `mimi plugin` — wraps `claude plugin`
- `mimi reflect` — prefrontal cortex: stops session, runs reflection agent, relaunches fresh
- `mimi audit` — reviews own codebase, proposes improvements via PR
- `mimi backup` — backup `~/.mimi/`
- `mimi config` — show config

### Key Rust files
- `src/main.rs` — CLI with clap, routes to commands
- `src/brain/mod.rs` — SQLite operations (entities, relationships, FTS search, stats, raw query)
- `src/brain/schema.sql` — DB schema (entities, relationships, memory_refs, FTS5, triggers, indexes)
- `src/claude.rs` — wraps `claude` CLI (launch_tmux, mcp, plugin, version)
- `src/dashboard/mod.rs` — axum web server with all API endpoints
- `src/paths.rs` — resolves `~/.mimi/` paths
- `src/commands/` — one file per command (setup, launch, status, brain, channel, config, backup, reflect, audit)
- `CLAUDE.md.template` — Mimi's identity/instructions, copied to `~/.mimi/CLAUDE.md` on setup

### SQLite Schema (`~/.mimi/brain.db`)
```sql
entities (id, type, name, properties JSON, created_at, updated_at)
relationships (id, source_id, target_id, type, properties JSON, created_at)
memory_refs (id, file_path, entity_id, created_at)
entities_fts — FTS5 virtual table with auto-sync triggers
```

### React Dashboard (`dashboard/`)
- React 19 + Vite + TailwindCSS 4 + shadcn (base-nova style)
- Built with `bun` (not npm)
- `bun install && bun run build` to build, output in `dashboard/dist/`
- The Rust server serves `dashboard/dist/` as static files
- shadcn components installed: button, card, badge, tabs, input, table, textarea, select, separator
- Uses `@` path alias for imports

### API Endpoints (axum, port 3131)
```
GET  /api/status                  — full status (session, brain stats, channels, memory count)
GET  /api/brain/stats             — entity/relationship counts by type
GET  /api/brain/entities?type=    — list entities, optional type filter
GET  /api/brain/search?q=         — FTS search entities
POST /api/brain/entities/add      — {type, name, properties}
POST /api/brain/relationships/add — {source_id, target_id, type}
POST /api/brain/query             — {sql} (SELECT/WITH only)
GET  /api/channels                — list channels
POST /api/channels/add            — {type, token?}
DELETE /api/channels/{name}       — remove channel
POST /api/channels/{name}/toggle  — enable/disable
POST /api/channels/{name}/configure — {token}
GET  /api/config                  — get config JSON
POST /api/config                  — save config JSON
POST /api/session/launch          — start tmux session
POST /api/session/stop            — kill tmux session
GET  /api/mcp/list                — list plugins
POST /api/backup                  — create backup
```

### Data directory (`~/.mimi/`)
```
brain.db          — SQLite knowledge graph
memory/           — markdown narrative memories + MEMORY.md index
accounts/         — credential JSON files (gitignored)
channels/         — channel config JSON files
config.json       — {name, model, session_name, dashboard_port}
CLAUDE.md         — Mimi's identity and brain instructions
backups/          — tar.gz backups
```

### Channels
Telegram, Discord, iMessage supported via Claude Code plugins. Adding a channel:
1. Installs the plugin (`claude plugin install telegram@claude-plugins-official`)
2. Creates config in `~/.mimi/channels/<type>.json`
3. Writes bot token to `~/.claude/channels/<type>/.env`
4. On launch, enabled channels are passed as `--channels plugin:<plugin-id>` to claude

### Nightly cron jobs (already configured)
- 3:00 AM: `mimi reflect` — self-reflection cycle
- 3:30 AM: `mimi audit` — codebase self-improvement

### GitHub
Repo: https://github.com/Larkooo/mimi-brain-interface

---

## YOUR TASK: Dashboard UI Redesign

The current dashboard is ugly and useless — it's a CRUD interface with entity forms, raw SQL queries, and generic shadcn cards. It needs to be completely redesigned as a brain visualization / neural monitoring interface.

### Design Direction
- **Dark, void-like background** — near-black with subtle blue tint
- **Force-directed graph** as the centerpiece — entities are glowing nodes, relationships are edges
- **Glassmorphism HUD overlays** — floating stat panels on top of the brain
- **3 views only**: Brain (default), Channels, Settings
- **No CRUD**: remove entity forms, relationship forms, raw SQL query panel
- **Mimi manages herself** — the dashboard is for observation and channel management

### New Architecture

**3 views** (replace 5 tabs):
1. **Brain** (default) — full-screen force-directed graph + floating HUD stats
2. **Channels** — channel management (add, configure, toggle, remove)
3. **Settings** — session control (start/stop) + config editor

**Navigation**: Thin left vertical rail (48px) with 3 icon-only buttons (brain, radio, settings). Active view gets a glowing cyan indicator. Use lucide-react icons.

### New Component Structure
```
src/
  App.tsx                    — View router + NavRail
  index.css                  — Neuralink dark theme overrides
  hooks/
    useApi.ts                — Add useGraph() hook
  components/
    NavRail.tsx              — Vertical icon nav
    brain/
      BrainView.tsx          — Full-screen container
      BrainGraph.tsx         — react-force-graph-2d with custom rendering
      NodeDetail.tsx         — Popover on node click
      StatsHUD.tsx           — Floating glassmorphism stat panels
    channels/
      ChannelsView.tsx       — Streamlined channel management
    settings/
      SettingsView.tsx       — Session control + config JSON editor
    ui/                      — Keep existing shadcn components
```

### Delete these old components
- `QueryPanel.tsx`, `BrainPanel.tsx`, `StatusPanel.tsx`, `ConfigPanel.tsx`, `Header.tsx`

### Backend: Add `/api/brain/graph` endpoint

Add to `src/brain/mod.rs`:
```rust
#[derive(Debug, Serialize)]
pub struct GraphNode {
    pub id: i64,
    pub name: String,
    pub r#type: String,
    pub properties: serde_json::Value,
    pub connections: usize,
}

#[derive(Debug, Serialize)]
pub struct GraphLink {
    pub source: i64,
    pub target: i64,
    pub r#type: String,
}

pub fn get_graph(db: &Connection) -> (Vec<GraphNode>, Vec<GraphLink>) {
    // Query entities with connection counts + all relationships
}
```

Add route in `src/dashboard/mod.rs`: `.route("/api/brain/graph", get(api_brain_graph))`

### Frontend: Brain Visualization

**Library**: `react-force-graph-2d` — install with `bun add react-force-graph-2d`

**Node rendering**:
- Each node = filled circle with radial gradient glow
- Color mapped by entity type:
  - person: `#00d4ff` (cyan)
  - company: `#863bff` (purple)
  - service: `#00ffa3` (teal)
  - concept: `#4d7cff` (blue)
  - account: `#ffb800` (amber)
  - project: `#ff3daa` (magenta)
  - location: `#ff6b35` (orange)
  - event: `#a0ff00` (lime)
- Size proportional to connection count (min 4px, max 20px)
- Labels only visible on hover or when zoomed in

**Edge rendering**: Thin lines (0.5px) at 20% opacity, brighten on hover

**Empty state**: Single pulsing node labeled "Mimi" with "waiting for thoughts..."

### Theme (index.css dark overrides)
```css
.dark {
  --background: oklch(0.05 0.01 270);     /* near-black + blue tint */
  --foreground: oklch(0.95 0 0);
  --card: oklch(0.08 0.005 270);
  --border: oklch(1 0 0 / 6%);
  --muted-foreground: oklch(0.45 0 0);
}
```

Add glass utility:
```css
.glass {
  background: rgba(255,255,255,0.03);
  backdrop-filter: blur(16px);
  border: 1px solid rgba(255,255,255,0.06);
}
```

### Floating HUD Panels (over brain graph)
- **Top-left**: Mimi name + pulsing status dot + claude version
- **Top-right**: Entity count / Relationship count / Memory files (mono font, large numbers)
- **Bottom-left**: Entity type legend with colored dots
- **Bottom-right**: Relationship type breakdown

All panels: `position: absolute`, glassmorphism, `pointer-events: none` on container.

### Implementation Order
1. Backend: Add `get_graph()` + `/api/brain/graph` endpoint
2. `bun add react-force-graph-2d`
3. Rewrite `index.css` dark theme
4. Create `NavRail.tsx`
5. Create `brain/BrainGraph.tsx` with custom node/edge rendering
6. Create `brain/StatsHUD.tsx` glassmorphism overlays
7. Create `brain/NodeDetail.tsx` popover
8. Create `brain/BrainView.tsx` composing graph + HUD
9. Create `channels/ChannelsView.tsx` (streamline existing)
10. Create `settings/SettingsView.tsx` (merge session control + config)
11. Rewrite `App.tsx` with NavRail + 3 views
12. Delete old components
13. `cargo build && cd dashboard && bun run build`
14. Restart dashboard: kill old process, run `./target/release/mimi dashboard`

### Build & Deploy
```bash
# Build Rust
cargo build --release

# Build React
cd dashboard && bun install && bun run build && cd ..

# Restart dashboard
pkill mimi; nohup ./target/release/mimi dashboard > /tmp/mimi-dashboard.log 2>&1 &
```

Dashboard serves on port 3131.
