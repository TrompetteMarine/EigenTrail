import Mathlib

namespace KGi.Lineage

/-- Exact real strict-above rank: the number of branches whose value at `t`
is strictly greater than the value of branch `i`. -/
noncomputable def rankAbove {n : ℕ} (f : Fin n → ℝ → ℝ)
    (i : Fin n) (t : ℝ) : ℕ := by
  classical
  exact (Finset.univ.filter (fun j => f i t < f j t)).card

/-- Target 1: a strict order reversal between two functions continuous on
`[a, b]` forces an equality at an interior time. -/
theorem order_reversal_has_collision
    {f g : ℝ → ℝ} {a b : ℝ}
    (hab : a < b)
    (hf : ContinuousOn f (Set.Icc a b))
    (hg : ContinuousOn g (Set.Icc a b))
    (hleft : f a < g a)
    (hright : g b < f b) :
    ∃ t ∈ Set.Ioo a b, f t = g t := by
  have hcont : ContinuousOn (fun t => f t - g t) (Set.Icc a b) := hf.sub hg
  have h0 : (0 : ℝ) ∈ Set.Icc (f a - g a) (f b - g b) :=
    ⟨by linarith, by linarith⟩
  obtain ⟨t, ht, hzero⟩ := intermediate_value_Icc hab.le hcont h0
  have hfg : f t = g t := by simp only at hzero; linarith
  refine ⟨t, ⟨lt_of_le_of_ne ht.1 ?_, lt_of_le_of_ne ht.2 ?_⟩, hfg⟩
  · rintro rfl; linarith
  · rintro rfl; linarith

/-- Helper: if `f i` never meets `f j` on `[a, b]`, the strict comparison
`f i t < f j t` is the same at every `t ∈ [a, b]` as at `a`. -/
theorem lt_iff_lt_of_no_collision
    {u v : ℝ → ℝ} {a b : ℝ}
    (hu : ContinuousOn u (Set.Icc a b))
    (hv : ContinuousOn v (Set.Icc a b))
    (hsep : ∀ t ∈ Set.Icc a b, u t ≠ v t) :
    ∀ t ∈ Set.Icc a b, (u t < v t ↔ u a < v a) := by
  intro t ht
  have hat : a ≤ t := ht.1
  have hsub : Set.Icc a t ⊆ Set.Icc a b := Set.Icc_subset_Icc le_rfl ht.2
  have hcont : ContinuousOn (fun s => u s - v s) (Set.Icc a t) :=
    (hu.sub hv).mono hsub
  have ha : a ∈ Set.Icc a b := ⟨le_rfl, hat.trans ht.2⟩
  constructor
  · intro h
    by_contra hna
    have hle : v a ≤ u a := not_lt.mp hna
    have h0 : (0 : ℝ) ∈ Set.Icc (u t - v t) (u a - v a) :=
      ⟨by linarith, by linarith⟩
    obtain ⟨s, hs, hzero⟩ := intermediate_value_Icc' hat hcont h0
    exact hsep s (hsub hs) (by simp only at hzero; linarith)
  · intro h
    by_contra hnt
    have hle : v t ≤ u t := not_lt.mp hnt
    have h0 : (0 : ℝ) ∈ Set.Icc (u a - v a) (u t - v t) :=
      ⟨by linarith, by linarith⟩
    obtain ⟨s, hs, hzero⟩ := intermediate_value_Icc hat hcont h0
    exact hsep s (hsub hs) (by simp only at hzero; linarith)

/-- Target 2: if branch `i` never collides with any other branch on `[a, b]`,
its strict-above rank is constant on `[a, b]`.

The ordering premise `a ≤ b` is kept as requested; the proof does not need it
(for `b < a` the interval is empty and the statement is vacuous), so its binder
is named `_hab`. Binder names do not affect the theorem's type. -/
theorem rank_above_constant
    {n : ℕ} {f : Fin n → ℝ → ℝ} {i : Fin n} {a b : ℝ}
    (_hab : a ≤ b)
    (hcontinuous : ∀ j, ContinuousOn (f j) (Set.Icc a b))
    (hseparated : ∀ j, j ≠ i →
      ∀ t ∈ Set.Icc a b, f i t ≠ f j t) :
    ∀ t ∈ Set.Icc a b, rankAbove f i t = rankAbove f i a := by
  classical
  intro t ht
  unfold rankAbove
  congr 1
  apply Finset.filter_congr
  intro j _
  by_cases hji : j = i
  · subst hji; simp
  · exact lt_iff_lt_of_no_collision (hcontinuous i) (hcontinuous j)
      (hseparated j hji) t ht

/-- Target 3: a change in strict-above rank between `a` and `b` forces a
collision of branch `i` with some other branch somewhere in `[a, b]`. -/
theorem rank_change_has_collision
    {n : ℕ} {f : Fin n → ℝ → ℝ} {i : Fin n} {a b : ℝ}
    (hab : a ≤ b)
    (hcontinuous : ∀ j, ContinuousOn (f j) (Set.Icc a b))
    (hrank : rankAbove f i a ≠ rankAbove f i b) :
    ∃ j, j ≠ i ∧ ∃ t ∈ Set.Icc a b, f i t = f j t := by
  by_contra hno
  push Not at hno
  exact hrank (rank_above_constant hab hcontinuous hno b ⟨hab, le_rfl⟩).symm

end KGi.Lineage
