"""Check three represented covariance designs; no observations are sampled.

The declared sample counts illustrate calibration behavior. They do not establish
that these matrices were estimated from that many Gaussian observations.
"""
import json
import math
import struct

from eigentrail import analyze_histories_json, check_histories_json


def encode(value):
    """Encode finite floats using the public canonical JSON format."""
    if isinstance(value, float):
        if not math.isfinite(value):
            raise ValueError("finite numerical values required")
        return "f64:" + struct.pack(">d", value).hex()
    if isinstance(value, str) and value.startswith(("f64:", "str:")):
        return "str:" + value
    if isinstance(value, list):
        return [encode(item) for item in value]
    if isinstance(value, dict):
        return {key: encode(item) for key, item in value.items()}
    return value


def decode(value):
    """Decode for display; retain the original JSON for certificate replay."""
    if isinstance(value, list):
        return [decode(item) for item in value]
    if isinstance(value, dict):
        return {key: decode(item) for key, item in value.items()}
    if isinstance(value, str) and value.startswith("str:"):
        return value[4:]
    if isinstance(value, str) and value.startswith("f64:"):
        return struct.unpack(">d", bytes.fromhex(value[4:]))[0]
    return value


def request(diagonal, sample_count):
    p = len(diagonal)
    # Matrix data are column-major; all entries here are exactly represented.
    matrix = {"n": p, "data": [float(diagonal[i]) if i == j else 0.0
                                for j in range(p) for i in range(p)]}
    observations = {
        "schema": "eigentrail.trace.v1",
        "times": [float(j) for j in range(10)],
        "covariances": [matrix for _ in range(10)],
        "features": [f"feature_{i}" for i in range(p)],
        # Supplied population regularity: C(t) is constant. Repeated input
        # matrices alone do not establish this assumption.
        "lipschitz": 0.0,
        "calibration": {"kind": "Gaussian", "observations": sample_count,
                        "estimated_mean": False, "divisor": float(sample_count),
                        "alpha": 0.05, "auxiliary_risk": 0.0},
        "correction": [0.0] * 10,
        "target": 0.3,
        "numerical_allowance": 1e-5,
        "memory_bytes": 512 * 1024**2,
    }
    return {"schema": "eigentrail.histories.v2", "observations": observations,
            "configurations": [{"id": "continuous", "lipschitz": 0.0,
                                "class": {"kind": "Continuous"}, "witnesses": []}],
            "query_times": [0.5]}


def run(label, diagonal, sample_count, expected_level):
    model = request(diagonal, sample_count)
    request_json = json.dumps(encode(model), sort_keys=True, separators=(",", ":"),
                              allow_nan=False)
    report_json = analyze_histories_json(request_json)
    assert check_histories_json(request_json, report_json) is True
    report = decode(json.loads(report_json))
    configuration = report["configurations"][0]
    base = configuration["continuous_conditional"]
    query = configuration["continuous_workflow"][0]["queries"][0]
    assert query["state"]["kind"] == "Bounds"
    block = query["state"]["blocks"][0]
    q = block["end"] - block["start"]
    p = len(diagonal)
    level = "whole-space" if q == p else "line" if q == 1 else "proper-subspace"
    tube = next(tube for tube in base["segments"][query["centre"]["segment"]]["tubes"]
                if (tube["start"], tube["end"]) == (block["start"], block["end"]))
    directional = (q < p and tube["informative"]
                   and block["projector_error"] < model["observations"]["target"])
    assert level == expected_level
    assert directional == (level != "whole-space")
    print(f"{label}: {level}, rank span {block['rank']}, "
          f"projector bound {block['projector_error']:.6g}, "
          f"directional target attained={directional}")
    print(f"  nodal radius {base['radii'][0]:.6g}; strength {block['strength']}")
    # Save request_json and report_json unchanged to replay this same certificate.


if __name__ == "__main__":
    run("Separated leading mode", [16.0, 1.0, 0.25], 32768, "line")
    run("Tied leading pair", [4.0, 4.0, 0.25], 32768, "proper-subspace")
    run("Smaller sample design", [16.0, 1.0, 0.25], 64, "whole-space")
    print("Checked numerical designs; Gaussian sampling and regularity remain premises.")
