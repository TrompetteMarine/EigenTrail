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

invalid <- tryCatch(track(covariances, c(0, 0, 1), radii, 0),
                    eigentrail_error = identity)
stopifnot(inherits(invalid, "eigentrail_error"))
