-- Run with: trace_processor_shell query -f scripts/core-lock-profile.sql capture.pftrace
-- Direct parent IDs pair each wait and hold on the same acquisition. Time-window
-- joins would incorrectly attribute simultaneous requests on other workers.
SELECT
  context.id AS acquisition,
  process.pid,
  thread.tid,
  thread.name AS worker,
  context.name AS caller,
  COALESCE(operation.name, context.name) AS operation,
  ROUND((wait.ts - trace_start()) / 1000000.0, 3) AS wait_start_ms,
  ROUND(wait.dur / 1000000.0, 3) AS wait_ms,
  ROUND((hold.ts - trace_start()) / 1000000.0, 3) AS hold_start_ms,
  ROUND(hold.dur / 1000000.0, 3) AS hold_ms
FROM slice AS context
JOIN slice AS wait ON wait.parent_id = context.id
  AND wait.name = 'GoVue core: lock_wait' AND wait.dur >= 0
LEFT JOIN slice AS hold ON hold.parent_id = context.id
  AND hold.name = 'GoVue core: lock_hold' AND hold.dur >= 0
LEFT JOIN slice AS operation ON operation.parent_id = hold.id
  AND operation.name GLOB 'GoVue core op:*' AND operation.dur >= 0
JOIN thread_track ON thread_track.id = context.track_id
JOIN thread USING (utid)
LEFT JOIN process ON process.upid = thread.upid
WHERE context.name GLOB 'GoVue core request:*' AND context.dur >= 0
ORDER BY wait.ts;
