library(eigentrail)

# These exact binary64 inputs describe a constant, diagonal two-feature path.
diagonal <- paste0('{"data":["f64:3fe8000000000000",',
                   '"f64:0000000000000000","f64:0000000000000000",',
                   '"f64:3fd0000000000000"],"n":2}')
request <- paste0(
    '{"calibration":{"assumptions":"Declared exact constant covariance path",',
    '"kind":"Conditional",',
    '"radii":["f64:0000000000000000","f64:0000000000000000"]},',
    '"correction":["f64:0000000000000000","f64:0000000000000000"],',
    '"covariances":[', diagonal, ',', diagonal, '],',
    '"features":["first","second"],"lipschitz":"f64:0000000000000000",',
    '"memory_bytes":67108864,"numerical_allowance":"f64:3f30000000000000",',
    '"schema":"eigentrail.trace.v1","target":"f64:3f90000000000000",',
    '"times":["f64:0000000000000000","f64:3ff0000000000000"]}')

exercise <- function(analyze, check, request, ...) {
    report <- analyze(request, ...)
    stopifnot(is.character(report), length(report) == 1L,
              isTRUE(check(request, ..., report)))
    tampered <- sub('"completed":true', '"completed":false', report, fixed = TRUE)
    stopifnot(!identical(tampered, report),
              inherits(tryCatch(check(request, ..., tampered), error = identity),
                       "eigentrail_error"),
              inherits(tryCatch(analyze("{", ...), error = identity),
                       "eigentrail_error"))
    report
}

trace <- exercise(track_json, check_json, request)
stopifnot(grepl('"matching":[0,1]', trace, fixed = TRUE))

jump_model <- paste0('{"at_most_one_per_cell":true,',
    '"minimum_applies_to_all_candidates":true,',
    '"minimum_jump":"f64:3fe0000000000000"}')
jumps <- exercise(analyze_jumps_json, check_jumps_json, request, jump_model)
stopifnot(grepl('"mandatory":[false]', jumps, fixed = TRUE),
          grepl('"optional":[false]', jumps, fixed = TRUE))

window <- paste0('{"attachments":[0,1],"block":[0,2],',
    '"bracket":["f64:3fe0000000000000","f64:3fe0000000000000"],',
    '"centre":0,"curvature":"f64:0000000000000000",',
    '"exact_collision_assumed":false,"secant":[0,1]}')
crossings <- exercise(analyze_crossings_json, check_crossings_json, request, window)
stopifnot(grepl('"outcome":{"gate":"StructuralPremise","kind":"Unresolved"}',
                crossings, fixed = TRUE))

history_request <- paste0('{"configurations":[{"class":{"kind":"Continuous"},',
    '"id":"constant","lipschitz":"f64:0000000000000000","witnesses":[]}],',
    '"observations":', request, ',"query_times":["f64:3fe0000000000000"],',
    '"schema":"eigentrail.histories.v2"}')
history <- exercise(analyze_histories_json, check_histories_json, history_request)
stopifnot(grepl('"kind":"OriginalAffineWitness"', history, fixed = TRUE))
stability <- analyze_stability_json(history_request, 0.125)
stopifnot(grepl('"eigentrail.stability.v1"', stability, fixed = TRUE),
          grepl('"StableWithinTolerance"', stability, fixed = TRUE))

frame_model <- paste0('{"common_frame_assumed":true,',
    '"weights":["f64:3ff0000000000000","f64:0000000000000000"]}')
common_frame <- exercise(analyze_common_frame_json, check_common_frame_json,
                         request, frame_model)
stopifnot(grepl('"possible_ranks":[[[1,1],[2,2]],[[1,1],[2,2]]]',
                common_frame, fixed = TRUE))

identity_frame <- paste0('{"data":["f64:3ff0000000000000",',
    '"f64:0000000000000000","f64:0000000000000000","f64:3ff0000000000000"],"n":2}')
transport_request <- paste0('{"block":[0,1],"dimension":2,',
    '"endpoint_error_limit":"f64:0000000000000000",',
    '"initial_correction_limit":"f64:0000000000000000",',
    '"initial_frame":', identity_frame, ',',
    '"initial_gauge":{"data":["f64:3ff0000000000000"],"n":1},',
    '"initial_mismatch_limit":"f64:0000000000000000",',
    '"max_speed":"f64:0000000000000000","memory_bytes":67108864,',
    '"nodes":[', identity_frame, ',', identity_frame, '],',
    '"reference_nodes":[', identity_frame, ',', identity_frame, '],',
    '"reference_speed_limit":"f64:0000000000000000",',
    '"schema":"eigentrail.transport.v1","target":"f64:3f90000000000000",',
    '"times":["f64:0000000000000000","f64:3ff0000000000000"]}')
transport <- exercise(analyze_transport_json, check_transport_json, transport_request)
stopifnot(grepl('"outcome":{"kind":"Resolved"}', transport, fixed = TRUE))

summary_request <- paste0('{"delta":"f64:3fc0000000000000","history":',
                         history_request, ',"schema":"eigentrail.lineage-summary.v1"}')
summary <- exercise(analyze_lineage_summary_json, check_lineage_summary_json,
                    summary_request)
stopifnot(grepl('"motion":"WithinTolerance"', summary, fixed = TRUE))
