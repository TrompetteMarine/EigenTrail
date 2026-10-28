# Canonical serialization

Canonical records are UTF-8 JSON with sorted object keys and no insignificant
whitespace. Finite binary64 values use `f64:` followed by exactly 16 lowercase
hexadecimal digits encoding the IEEE-754 bits, preserving signed zero. Literal
strings with reserved prefixes are escaped using `str:`. Counts are integers.
Matrices are column-major: concatenate the columns. SHA-256 binds canonical bytes.

NaN, infinity, null numerical values, duplicate keys, unknown fields and
unsupported schema versions are rejected. Undefined or unbounded quantities use
explicit tagged objects. Intervals declare endpoint inclusion; jump cells use
(a,b]. A migrated record requires rechecking before relying on its certificate.

Trace records use `eigentrail.trace.v1`. Histories use their explicit supported
`eigentrail.histories.v1` or `eigentrail.histories.v2` version. Models and reports
for jumps, crossings and transport declare their own schema fields. Preserve the
original canonical request/report pair for the corresponding check route.

`eigentrail.lineage-summary.v1` wraps a complete v2 history request and a finite
positive `delta`. Its report embeds the checked history and ordered summary rows.
The outer hash binds the whole request, including `delta`; the inner history
retains its own request hash. All numerical values use the same canonical bits.

Python's `track` and `track_history` convenience methods decode these finite values
for presentation. Use the `*_json` interfaces for exact byte-preserving replay;
ordinary `json.dumps` on a decoded presentation dictionary is not that wire format.
