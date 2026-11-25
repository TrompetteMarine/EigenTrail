\name{rolling_lineage_json}
\alias{rolling_lineage_json}
\alias{analyze_rolling_lineage_json}
\alias{check_rolling_lineage_json}
\title{Raw canonical JSON rolling lineage analysis and checking}
\description{
Analyze a canonical rolling lineage request and independently check a rolling
lineage report. Both functions accept and return the public canonical wire
format only; all numerical work, admission and checking happen in native Rust.
No numeric work is done in R.
}
\usage{
analyze_rolling_lineage_json(request)
check_rolling_lineage_json(request, report)
}
\arguments{
\item{request}{One non-NA character string holding a canonical rolling lineage request.}
\item{report}{One non-NA character string holding a canonical rolling lineage report.}
}
\details{
Inputs are observed-date symmetric positive definite matrices expressed in one
fixed feature chart; no interpolation between dates, chart change or
reparameterization is performed.

The angular classification uses equality-inclusive tolerance: an angle is
within tolerance when its certified upper bound \eqn{U \le} tolerance. This
differs from the existing strict lineage displacement comparison.

Point mode describes the supplied matrices; numerical enclosure uncertainty
remains explicit. Conditional mode declares a Thompson-region radius, joint
event identity and radius provider. A supplied radius has no implied sampling
level. These premises are retained as supplied and are not inferred from data.

No guarantee is given for an internal tied line: where eigenvalues tie inside
the analyzed span, lineage identity is not certified.
}
\value{
\code{analyze_rolling_lineage_json} returns one canonical checked report JSON
string. \code{check_rolling_lineage_json} returns \code{TRUE} when the report
checks against the request; failures raise an \code{eigentrail_error}
condition.
}
