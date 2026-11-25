# Container Image Manifest Specification

> Status: Historical design summary.
> Original source (no longer tracked): `docs/IMAGE_MANIFEST.md`.
> For current packaging instructions, see [container builds](../../../README.md#build-the-container) and the [release checklist](../../../RELEASING.md#release-checklist).

## Problem

Image metadata is minimal and can be overlooked during release checks.

## Goal

Normalize the image manifest into a spec artifact that can be referenced in release and deployment workflows.

## Scope

### In Scope

- Image identity and manifest-level metadata
- Traceability to deployment/release process

### Out of Scope

- Full deployment playbook
- Runtime tuning and environment-specific overrides

## Design Summary

- Source is intentionally concise; this spec preserves that as a lightweight contract.
- Manifest remains a reference point for image naming and packaging consistency.

## Success Criteria

- Image identity is explicit and stable for release automation.
- Manifest can be referenced by checklist and CI/release docs.
