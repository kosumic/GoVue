-- Run with: trace_processor_shell query -f scripts/transcript-profile.sql capture.pftrace
WITH requests AS (
  SELECT ts, dur, name FROM slice
  WHERE name LIKE 'GoVue transcript: request [%' AND dur >= 0
)
SELECT
  request.name AS request,
  stage.name AS stage,
  ROUND((stage.ts - request.ts) / 1000000.0, 3) AS start_ms,
  ROUND(stage.dur / 1000000.0, 3) AS duration_ms
FROM requests AS request
JOIN slice AS stage ON stage.ts >= request.ts
  AND stage.ts + stage.dur <= request.ts + request.dur
WHERE stage.dur >= 0
  AND (stage.name GLOB 'GoVue transcript:*' OR stage.name GLOB 'GoVue core:*')
ORDER BY request.ts, stage.ts, stage.dur DESC;
