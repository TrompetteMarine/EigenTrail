library(eigentrail)

covariances <- array(0, dim = c(2, 2, 3))
for (j in seq_len(3)) covariances[, , j] <- diag(c(2, 1))
times <- c(0, 0.5, 1)
radii <- rep(0, 3)
report <- track(covariances, times, radii, lipschitz = 0)
stopifnot(is.character(report), length(report) == 1L,
          grepl("eigentrail.trace.v1", report, fixed = TRUE),
          is.finite(attr(report, "caller_owned_payload_estimate_bytes")))

history <- track_history(covariances, times, radii, lipschitz = 0,
                         query_times = c(0.25, 0.75))
stopifnot(is.character(history), length(history) == 1L,
          grepl("eigentrail.histories.v2", history, fixed = TRUE),
          grepl("continuous_workflow", history, fixed = TRUE))

radius <- 2^-20
candidate <- array(rep(c(0.75, 0, 0, 0.25), 2), dim = c(2, 2, 2))
observed <- candidate
observed[1, 1, 2] <- observed[1, 1, 2] + radius / 2
baseline <- track_history(observed, c(0, 1), rep(radius, 2), 0, c(0, 0.5, 1))
verified <- track_history(observed, c(0, 1), rep(radius, 2), 0, c(0, 0.5, 1),
                          witness = candidate)
stopifnot(grepl('"feasibility":{"kind":"Unresolved"}', baseline, fixed = TRUE),
          grepl('"feasibility":{"kind":"OriginalAffineWitness"}', verified,
                fixed = TRUE),
          attr(verified, "caller_owned_payload_estimate_bytes") ==
              attr(baseline, "caller_owned_payload_estimate_bytes") +
              8 * length(candidate))
outside <- candidate
outside[1, 1, ] <- 1
unresolved <- track_history(observed, c(0, 1), rep(radius, 2), 0, c(0.5),
                            witness = outside)
stopifnot(grepl('"feasibility":{"kind":"Unresolved"}', unresolved, fixed = TRUE))

for (invalid_witness in list(candidate[, , 1], array(1L, dim(candidate)))) {
    invalid <- tryCatch(track_history(observed, c(0, 1), rep(radius, 2), 0, c(0.5),
                                      witness = invalid_witness),
                        eigentrail_invalid = identity)
    stopifnot(inherits(invalid, "eigentrail_invalid"))
}

invalid <- tryCatch(track(covariances, c(0, 0, 1), radii, 0),
                    eigentrail_error = identity)
stopifnot(inherits(invalid, "eigentrail_error"))

stability <- track_stability(covariances, times, radii, lipschitz = 0,
                            query_times = c(0.25, 0.75), tolerance = 0.01)
stopifnot(is.character(stability), length(stability) == 1L,
          grepl("eigentrail.stability.v1", stability, fixed = TRUE),
          grepl("StableWithinTolerance", stability, fixed = TRUE),
          grepl("calibration_scope", stability, fixed = TRUE),
          is.finite(attr(stability, "caller_owned_payload_estimate_bytes")))

for (tolerance in list(NULL, NaN, Inf, -0.1, 1.1, c(0.1, 0.2))) {
    invalid_stability <- tryCatch(
        track_stability(covariances, times, radii, 0, c(0.5), tolerance),
        eigentrail_error = identity)
    stopifnot(inherits(invalid_stability, "eigentrail_error"))
}



window <- window_envelope(c(0, 2), c(1, 3), 0.5, 0.125, 0.5, 0.25,
                          "Declared weighted population target B",
                          "Conditional source event and global regularity")
stopifnot(is.character(window), length(window) == 1L,
          grepl("eigentrail.window-envelope.v1", window, fixed = TRUE),
          grepl("mean_absolute_offset", window, fixed = TRUE),
          grepl("Declared weighted population target B", window, fixed = TRUE))
for (weights in list(c(0, 0), c(-1, 3), c(Inf, 1), c(1))) {
    invalid_window <- tryCatch(
        window_envelope(c(0, 2), weights, 0.5, 0.125, 0.5, 0.25,
                         "Declared target", "Conditional event"),
        eigentrail_error = identity)
    stopifnot(inherits(invalid_window, "eigentrail_error"))
}
