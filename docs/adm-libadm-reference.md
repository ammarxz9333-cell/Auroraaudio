# ADM libadm reference lane

Aurora does not currently claim native or production ADM XML ingestion. This lane uses EBU `libadm` only as an exact-pin external parser/writer validation reference for bounded ADM infrastructure work.

## Exact external reference

| Reference | Exact commit | Upstream project version | Aurora role |
| --- | --- | --- | --- |
| EBU `libadm` | `aed1ac5aea2844da374712a2b51339a09485df6f` | `0.14.0` | external ADM XML parse/write and malformed-input reference |

The upstream source is Apache-2.0. The reference is checked out and built in CI; it is not vendored, linked into Aurora core, or exposed as an Aurora runtime parser.

## Validation gate

The CI lane:

1. checks out the exact pinned commit and verifies Aurora's registry/config pin;
2. builds `libadm` with upstream unit tests and examples enabled;
3. runs the complete upstream CTest suite;
4. uses upstream `create_from_scratch` to generate an ADM document;
5. sends that document through upstream `parse_xml` twice;
6. requires the second parse/write pass to preserve selected ADM element counts and a whitespace/prefix-insensitive structural digest of `audioFormatExtended`;
7. requires minimum programme/content/object structure in the round-tripped document; and
8. feeds a CI-local truncated XML document to the parser and requires a non-zero exit status.

The evidence artifact records the Aurora SHA, exact `libadm` commit/version, XML hashes, structural hashes, selected element counts, malformed-input exit code, verdict, and truth boundary.

The lane intentionally does not require byte-identical XML. Parsing and writing can normalize whitespace, namespace presentation, or serialization details without changing the bounded ADM structure under test.

## What a pass means

A pass means only that the pinned upstream `libadm` revision builds, its upstream unit tests pass, and its own create/parse/write path preserves the bounded ADM structure tested by Aurora while rejecting the malformed XML probe.

It does **not** prove:

- Aurora runtime ADM ingestion;
- BW64 audio/data integration;
- arbitrary ADM scene-to-Aurora mapping;
- completeness for every ITU-R BS.2076 feature or sub-element;
- S-ADM / ITU-R BS.2125 support;
- ITU-R BS.2127 rendering semantics;
- object, DirectSpeakers, HOA, or matrix rendering equivalence;
- 7.1.4 or 11.1.4 renderer behavior;
- protected-service interoperability;
- physical output behavior; or
- certification.

Upstream itself documents that some ADM sub-elements are not implemented and that S-ADM is not supported. Aurora therefore keeps those capabilities outside this lane's evidence boundary.

## Next ADM step

After this parser/writer gate is stable, renderer validation stays separate. EBU EAR/libear and SAF can be introduced as independent external rendering/DSP oracles, with layout/object/HOA evidence kept distinct from `libadm` parser evidence.
