# Repository History

## Canonical History

Aurora temporarily had two unrelated Git histories: a documentation-first
history already hosted on GitHub and a local implementation history containing
the formally accepted Simulation Sprint 1 source tree. Git confirmed that the
histories have no common ancestor.

The accepted implementation is authoritative for continued development:

- accepted implementation branch: `main-v2`
- accepted implementation commit: `637748e1a95eefa09db26f0485cc4d3a6de7c105`
- accepted tag: `simulation-sprint-1-accepted`
- source of truth: `AURORA_MASTER_REFERENCE.md`

`main-v2` was created directly from the accepted annotated tag. The tag was not
moved or recreated.

## Legacy History

The previous GitHub history is preserved at its original head,
`4d166cc14f759e0d2eb8f2614df1b923aed09388`, on both:

- `main`
- `legacy/pre-master-reference`

These branches are historical references. They are not ancestors of `main-v2`
and must not be merged into canonical development. Potentially useful legacy
material is reviewed by content, with `AURORA_MASTER_REFERENCE.md` retaining
authority over all decisions and milestone scope.

## Integration Decision

The histories were intentionally kept separate. The integration used ordinary
branch and tag pushes only:

- no force push;
- no rebase or history rewrite;
- no unrelated-history merge;
- no deletion or renaming of the previous `main` branch;
- no modification of the accepted Simulation Sprint 1 commit or tag.

GitHub uses `main-v2` as the default branch. The legacy branches remain
available for audit and selective, documented review.
