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
#' @details Optional anchor weights select a declared common frame for the entire path.
#' @export
track_history <- function(covariances, times, radii, lipschitz, query_times,
                          common_frame_weights=NULL) {
    report <- .checked(.Call(wrap__track_history_covariances, covariances, times,
                            radii, lipschitz, query_times, common_frame_weights))
    attr(report,"caller_owned_payload_estimate_bytes") <- 8*(length(covariances)+
        length(times)+length(radii)+length(query_times)+length(common_frame_weights))
    report
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
