# Pending steering remains wakeable across a tool batch

## Contract and evidence

Issue #897; baseline `99182cfc0a4d343101f1b41af1cb423615997dc2`.
`QueueSteeringSource::ready` observes pending metadata, not acceptance.
The runtime consumes steering only after its sequential tool batch finishes.
`Registry::delivered_steering` currently suppresses subsequent waits for the same
pending ID, so a second wait can prevent the batch from reaching acceptance.
This is a statically established path, not proof of the original screenshot's
exact execution or installed build.

## Acceptance (before implementation)

1. In a real runtime response batch containing two, or three repeated waits for
   a held running execution, enqueue valid steering after the first wait has
   observed an empty queue. Every wait finishes with a valid receipt, before
   the execution completes. The next provider request contains every tool
   result followed by the steering message exactly once, with original content.
2. Repeated explicit observation deadlines may coalesce; pending steering still
   wakes every subsequent wait without renewing the deadline allowance.
3. After real queue acceptance, an additional wait stays pending without a new
   message. A new valid message wakes it; old content is not delivered twice.
4. Ordinary queue entries, wrong-generation entries and permission-invalidated
   entries do not wake steering. No-message waits retain their timeout and
   released-terminal behavior.
5. Waking a parent wait leaves the child's token and held ownership unchanged.
   Cancelling a wait still returns cancellation; terminal release remains
   distinct from steering.

## Minimal implementation plan

Remove `RegistryState::delivered_steering` and `Registry::steering_ready`.
In `Hook::call` return steering whenever the existing source reports a pending
ID. Keep readiness polling, target validation, cancellation, attention and
terminal priority, deadlines and coalescing unchanged. Do not change queue
admission or move the runtime's acceptance boundary.

## Validation design and non-goals

Development-only Rust regressions use the production runtime loop, wait hook
and persisted queue; a scripted in-process Provider isolates the network.
A readiness observer provides deterministic empty-queue synchronization.
A held registry execution isolates child execution without completing it to
unblock the batch. Assertions cover receipts, next request ordering and queue
claiming, not source text. Bounded timeouts only guard hangs.

No UI change, tool-between-tool preemption, new notification system, scheduler
refactor, general shell interruption, or automatic child cancellation.

Local validation is limited to format, diff and static inspection. Run behavior
regressions in existing PR Actions, preserve their exact source commit and
results, then remove Rust tests and exclusive support before final required CI
under repository policy. Final zero-test CI is not behavior-test evidence.
Non-writer independent review and final-head required CI precede squash merge;
Issue remains open In review pending the user's real-App manual acceptance.
