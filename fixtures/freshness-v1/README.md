# Research Provenance Fixture: freshness-v1

> Status: **additive Phase 1 Task 0 conformance fixture**
>
> Nature: **hand-audited, synthetic, and non-normative for scientific facts**

This fixture freezes deterministic `rp/freshness/v1` query behavior without
changing any of the 176 G7 canonical objects. Its canonical `valid/` project has
one Project, two exact ExternalReferences representing separately tracked source
versions, and five Question NodeRevisions.

At `as_of: 2027-01-01T00:00:00Z`, the oracle covers:

- `fresh`: complete applicable checks and a future review deadline;
- `review-due`: no stale condition and a reached review deadline;
- `stale`: expired `effective_until`, including precedence over a future review;
- `stale`: a newer separately tracked source version under
  `invalidates_on_change` policy;
- `unknown`: an applicable required source check is unavailable.

`expected-freshness.yaml` includes both positive classifications and explicit
negative assertions that prevent boundary or precedence inversions. An earlier
`as_of` case also proves that a source version released in the future relative
to the query cannot make the object stale.

The policy file is validated by `freshness-policy.schema.json`; the oracle is
validated by `fixture-expectations.schema.json`. Neither schema is part of the
canonical Layer B dispatch.
