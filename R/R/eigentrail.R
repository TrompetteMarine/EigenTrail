#' Track covariance stacks in fixed feature coordinates
#' @param covariances Double array (features, features, dates).
#' @param times Strictly increasing finite double vector.
#' @param radii Conditional nodal covariance error bounds.
#' @param lipschitz Declared global covariance regularity bound.
#' @return Canonical checked TRACE JSON. No statistical assumption is inferred.
#' @export
.checked <- function(value) {
    if (inherits(value,"extendr_error")) {
        message <- as.character(value$value)
        kind <- tolower(sub("[(:].*$","",message))
        stop(structure(list(message=message,call=NULL,kind=kind),
            class=c(paste0("eigentrail_",kind),"eigentrail_error","error","condition")))
    }
    value
}
track <- function(covariances, times, radii, lipschitz) {
    report <- .checked(.Call(wrap__track_covariances, covariances, times, radii, lipschitz))
    attr(report,"caller_owned_payload_estimate_bytes") <- 8*(length(covariances)+length(times)+length(radii))
    report
}
#' Track a continuous path at observation and query times
#' @param query_times Finite double vector inside the observation domain.
#' @param common_frame_weights Nonnegative double anchor weights, one per date.
#' @param witness Optional double array matching covariances. Supplies one
#'   candidate matrix per observation date with zero radius widening.
#' @details Optional anchor weights select a declared common frame for the entire path.
#' @export
track_history <- function(covariances, times, radii, lipschitz, query_times,
                          common_frame_weights=NULL, witness=NULL) {
    report <- .checked(.Call(wrap__track_history_covariances, covariances, times,
                            radii, lipschitz, query_times, common_frame_weights,
                            witness))
    attr(report,"caller_owned_payload_estimate_bytes") <- 8*(length(covariances)+
        length(times)+length(radii)+length(query_times)+length(common_frame_weights)+
        length(witness))
    report
}
#' Classify checked projector movement against a prespecified tolerance
#' @param tolerance One finite double in [0, 1], in projector operator distance.
#' @details Retains the complete source history, including uncertainty, identity
#'   qualifications and feasibility. Projector stability does not establish
#'   covariance equality or unchanged temporal dynamics.
#' @export
track_stability <- function(covariances, times, radii, lipschitz, query_times,
                            tolerance, witness=NULL) {
    if (is.null(tolerance)) stop(structure(
        list(message="stability tolerance required", call=NULL, kind="invalid"),
        class=c("eigentrail_invalid", "eigentrail_error", "error", "condition")))
    report <- .checked(.Call(wrap__track_stability_covariances, covariances, times,
                            radii, lipschitz, query_times, tolerance, witness))
    attr(report,"caller_owned_payload_estimate_bytes") <- 8*(length(covariances)+
        length(times)+length(radii)+length(query_times)+length(witness)+1)
    report
}

#' Transfer a declared window-target radius to instantaneous covariance
#' @details This deterministic transfer requires an explicitly declared estimator
#'   target and source error event. It does not calibrate temporal uncertainty.
#'   The returned total radius may be used as a conditional TRACE radius without
#'   charging its included mismatch again through correction.
#' @export
window_envelope <- function(sample_times, weights, target_time, window_radius,
                            lipschitz, target_mismatch_bound, target_description,
                            assumptions) {
    .checked(.Call(wrap__window_envelope_json, sample_times, weights, target_time,
                   window_radius, lipschitz, target_mismatch_bound,
                   target_description, assumptions))
}

#' Bound a window second moment under declared temporal Gaussian premises
#' @details Requires jointly Gaussian known-mean residuals, uniform weights and
#'   a prespecified block covariance envelope. Use the same total window count
#'   and alpha across a prespecified family for simultaneous coverage. The
#'   assumptions string is retained; this scalar API does not verify its truth.
#' @export
temporal_gaussian <- function(dimension, observations, windows, covariance_bound,
                              correlation_bound, alpha, assumptions) {
    .checked(.Call(wrap__temporal_gaussian_json, dimension, observations, windows,
                   covariance_bound, correlation_bound, alpha, assumptions))
}
#' Convert a conditional operator tube to SPD geometry and precision bounds
#' @details The positive centre eigenvalue floor is supplied, not verified.
#'   Radii reaching that floor return unresolved geometry. This deterministic
#'   conversion creates no probability event or whole-curve witness.
#' @export
spd_tube_geometry <- function(dimension, centre_floor, radius) {
    .checked(.Call(wrap__spd_tube_geometry_json, dimension, centre_floor, radius))
}
#' @export
track_json <- function(request) .checked(.Call(wrap__track_json, request))
#' @export
check_json <- function(request, report) .checked(.Call(wrap__check_json, request, report))
#' @export
analyze_jumps_json <- function(request, model) .checked(.Call(wrap__analyze_jumps_json, request, model))
#' @export
check_jumps_json <- function(request, model, report) .checked(.Call(wrap__check_jumps_json, request, model, report))
#' @export
analyze_crossings_json <- function(request, window) .checked(.Call(wrap__analyze_crossings_json, request, window))
#' @export
check_crossings_json <- function(request, window, assessment) .checked(.Call(wrap__check_crossings_json, request, window, assessment))
#' @export
analyze_histories_json <- function(request) .checked(.Call(wrap__analyze_histories_json, request))
#' @export
analyze_stability_json <- function(request, tolerance) .checked(.Call(wrap__analyze_stability_json, request, tolerance))
#' @export
check_histories_json <- function(request, report) .checked(.Call(wrap__check_histories_json, request, report))
#' @export
analyze_common_frame_json <- function(request, model) .checked(.Call(wrap__analyze_common_frame_json, request, model))
#' @export
check_common_frame_json <- function(request, model, report) .checked(.Call(wrap__check_common_frame_json, request, model, report))

#' Analyze initialized exact-polar geometry separately from covariance uncertainty
#' @export
analyze_transport_json <- function(request) .checked(.Call(wrap__analyze_transport_json, request))
#' @export
check_transport_json <- function(request, assessment) .checked(.Call(wrap__check_transport_json, request, assessment))

#' Summarize checked continuous/common-frame histories with separate motion and accuracy
#' @export
analyze_lineage_summary_json <- function(request) .checked(.Call(wrap__analyze_lineage_summary_json, request))
#' @export
check_lineage_summary_json <- function(request, report) .checked(.Call(wrap__check_lineage_summary_json, request, report))
