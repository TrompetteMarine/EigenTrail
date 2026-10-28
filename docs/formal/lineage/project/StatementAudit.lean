import RequestProject.Lineage

/-!
Acceptance assertions stated from the original request, independently of the
returned proof bodies. These check scalar statement types and the exact finite
counting definition. They do not assert any matrix or native-software theorem.
-/

example :
    ∀ {f g : ℝ → ℝ} {a b : ℝ},
      a < b →
      ContinuousOn f (Set.Icc a b) →
      ContinuousOn g (Set.Icc a b) →
      f a < g a →
      g b < f b →
      ∃ t ∈ Set.Ioo a b, f t = g t :=
  @KGi.Lineage.order_reversal_has_collision

example :
    ∀ {n : ℕ} {f : Fin n → ℝ → ℝ} {i : Fin n} {a b : ℝ},
      a ≤ b →
      (∀ j, ContinuousOn (f j) (Set.Icc a b)) →
      (∀ j, j ≠ i → ∀ t ∈ Set.Icc a b, f i t ≠ f j t) →
      ∀ t ∈ Set.Icc a b,
        KGi.Lineage.rankAbove f i t = KGi.Lineage.rankAbove f i a :=
  @KGi.Lineage.rank_above_constant

example :
    ∀ {n : ℕ} {f : Fin n → ℝ → ℝ} {i : Fin n} {a b : ℝ},
      a ≤ b →
      (∀ j, ContinuousOn (f j) (Set.Icc a b)) →
      KGi.Lineage.rankAbove f i a ≠ KGi.Lineage.rankAbove f i b →
      ∃ j, j ≠ i ∧ ∃ t ∈ Set.Icc a b, f i t = f j t :=
  @KGi.Lineage.rank_change_has_collision

example :
    ∀ {u v : ℝ → ℝ} {a b : ℝ},
      ContinuousOn u (Set.Icc a b) →
      ContinuousOn v (Set.Icc a b) →
      (∀ t ∈ Set.Icc a b, u t ≠ v t) →
      ∀ t ∈ Set.Icc a b, (u t < v t ↔ u a < v a) :=
  @KGi.Lineage.lt_iff_lt_of_no_collision

example {n : ℕ} (f : Fin n → ℝ → ℝ) (i : Fin n) (t : ℝ) :
    KGi.Lineage.rankAbove f i t =
      (Finset.univ.filter (fun j => f i t < f j t)).card := by
  classical
  rfl
