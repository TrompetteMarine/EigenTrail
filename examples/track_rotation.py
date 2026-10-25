"""Run with an installed EigenTrail and NumPy: python examples/track_rotation.py."""
import math

import numpy as np

import eigentrail


def main():
    times = np.linspace(0.0, 1.0, 101)
    covariances = np.empty((len(times), 2, 2), dtype=np.float64)
    for j, t in enumerate(times):
        c, s = math.cos(0.1 * t), math.sin(0.1 * t)
        a, b = 0.8 + 0.01 * t, 0.2 - 0.01 * t
        off_diagonal = (a - b) * s * c
        covariances[j] = [[a*c*c + b*s*s, off_diagonal],
                          [off_diagonal, a*s*s + b*c*c]]

    # Declared error/regularity bounds for this known synthetic covariance path.
    report = eigentrail.track(covariances, times, np.full(len(times), 1e-6), lipschitz=0.072)
    for segment in report["segments"]:
        leading = segment["tubes"][0]
        assert leading["start"] == 0 and leading["end"] == 1 and leading["informative"]
        assert segment["association"] == {"kind": "Unique", "matching": [0, 1]}

    values = np.array([e["values"] for e in report["eigensystems"]])
    # Independent analytical eigenvalues for this example, rather than a saved output.
    expected = np.column_stack([0.8 + 0.01 * times, 0.2 - 0.01 * times])
    assert np.max(np.abs(values - expected)) < 1e-12
    directions = []
    previous = np.array([1.0, 0.0])
    for eigen in report["eigensystems"]:
        axis = np.array(eigen["vectors"]["data"]).reshape((2, 2), order="F")[:, 0]
        if np.dot(axis, previous) < 0:
            axis = -axis
        directions.append(math.degrees(math.atan2(axis[1], axis[0])))
        previous = axis
    assert np.max(np.abs(np.array(directions) - np.degrees(0.1 * times))) < 1e-10

    print("Tracking a slowly rotating 2-feature covariance path")
    print(" time   leading value   other value   leading share   direction")
    for j in [0, 25, 50, 75, 100]:
        share = values[j, 0] / values[j].sum()
        print(f" {times[j]:.2f}      {values[j, 0]:.4f}         {values[j, 1]:.4f}"
              f"         {share:.1%}         {directions[j]:.2f} deg")
    bound = max(segment["tubes"][0]["bound"] for segment in report["segments"])
    print(f"All {len(report['segments'])} intervals retain a unique leading-mode association.")
    print(f"Maximum leading-projector error bound: {bound:.6f} (target 0.002).")
    print("Values and directions above are estimates; the bound uses the declared model assumptions.")


if __name__ == "__main__":
    main()
