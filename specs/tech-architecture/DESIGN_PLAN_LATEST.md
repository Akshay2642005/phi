# Design Plan — AI Workflow Engine

## Design Principles

- **Generic over I:** Workflow<I> works with any input type (Send + Sync + Debug + 'static)
- **DAG-driven:** Skills registered → dependency resolution → DAG planning → execution
- **Provider-agnostic LLM:** LlmService trait abstracts over LLM providers
- **Event-driven:** EventBus with broadcast channel for workflow lifecycle events
- **Plugin system:** Plugin<I> trait for extensibility

## Key Design Decisions

1. **Strong typing:** UUID-based newtypes for all IDs (WorkflowId, ExecutionId, NodeId)
2. **Petgraph encapsulation:** WorkflowGraph wraps petgraph; no other module imports it
3. **Skill trait + Registry:** Skill<I> defines unit of work; Registry manages registration + topological sort
4. **ReAct as runtime layer:** PlannerSkill (LLM reasoning) + tool skills, not baked into core engine
5. **Sequential execution:** Current engine runs one skill at a time; parallel prepared but not enabled
