# Canonical JSON format

Use JSON interfaces when saving a request and report for later checking. Preserve
their original strings; Python's decoded presentation dictionaries are not the
wire format.

## Values and ordering

Records are UTF-8 JSON with sorted object keys and no insignificant whitespace.
Finite binary64 values use `f64:` followed by exactly 16 lowercase hexadecimal
digits for their IEEE-754 bits, preserving signed zero. For example, `0.5` becomes
`"f64:3fe0000000000000"`. Counts remain JSON integers. Strings beginning with
`f64:` or `str:` are escaped by prepending `str:`.

Matrices use column-major data: concatenate the columns. A matrix is represented
as `{"n": p, "data": [...]}`, with each numerical entry encoded as above.
SHA-256 request hashes bind canonical bytes.

NaN, infinity, null numerical values, duplicate keys, unknown fields and
unsupported schema versions are rejected. Undefined and unbounded quantities
have explicit tagged objects. Intervals carry endpoint-inclusion flags; jump
cells use `(a,b]`.

## Schema and checking

Trace records use `eigentrail.trace.v1`. Histories declare
`eigentrail.histories.v1` or `eigentrail.histories.v2`; jumps, crossings and
transport have their own schema fields. Use the corresponding check function
with the same canonical inputs used for analysis. See the [API guide](api.md).
A converted or migrated record must be checked again before relying on it.

`eigentrail.lineage-summary.v1` wraps a complete v2 history request in `history`
and a finite positive `delta`. Its report contains the checked history and ordered
summary rows. The outer `request_hash` binds the complete request, including
`delta`; the nested history retains its own request hash.

The [request examples](../examples/requests/) provide ready-to-use canonical
inputs. [Gaussian history](../examples/gaussian-history.py) also shows a small
Python encoder and a display decoder. Saving ordinary `json.dumps` output from a
decoded report does not preserve its certificate encoding.
