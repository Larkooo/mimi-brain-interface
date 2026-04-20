import { useState } from 'react'
import type { Status, GraphData, GraphNode } from '../../hooks/useApi'
import { BrainGraph } from './BrainGraph'
import { StatsHUD } from './StatsHUD'
import { NodeDetail } from './NodeDetail'

export function BrainView({ status, graph }: { status: Status | null; graph: GraphData | null }) {
  const [selectedNode, setSelectedNode] = useState<GraphNode | null>(null)

  return (
    <div className="absolute inset-0 overflow-hidden bg-background">
      <BrainGraph graph={graph} onNodeClick={setSelectedNode} />
      <StatsHUD status={status} stats={status?.brain_stats ?? null} />
      {selectedNode && (
        <NodeDetail node={selectedNode} onClose={() => setSelectedNode(null)} />
      )}
    </div>
  )
}
