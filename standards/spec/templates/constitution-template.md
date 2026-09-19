# &lt;project&gt; constitution

Durable principles that govern this corpus. **Tier 2**: subordinate to the
bootstrap spec (`specs/000-*/spec.md`) and governing all ordinary specs.

**Normative hierarchy (highest wins):**

1. `specs/000-*/spec.md`: the bootstrap spec. Non-overridable.
2. `standards/spec/constitution.md`: this document.
3. `standards/spec/contract.md`: normative summary of the bootstrap spec.
4. Ordinary specs (`001`+).

This hierarchy is an authority ordering. It says which text wins when two texts
disagree; it says nothing about the order in which branches merge.

---

## I. &lt;Principle name&gt;

&lt;One paragraph. State the principle as a durable rule, and cite the bootstrap
anchor it rests on, if any.&gt;

## II. &lt;Principle name&gt;

&lt;...&gt;

## III. &lt;Principle name&gt;

&lt;...&gt;

---

## Amendment

This constitution is changed by an ordinary spec that is `approved`, **claims the
affected text as an authority unit of this file**, and contradicts no anchor in
the `unamendable` list of `specs/000-*`.

The claim uses the ordinary ownership vocabulary over a **section unit of this
file**:

- `establishes` a `{ kind: section, file: "standards/spec/constitution.md",
  anchor: <heading-slug> }` unit for a principle the spec adds;
- `refines` that unit, with a named `aspect`, for a principle it tightens;
- `co_authority` on that unit where a principle is genuinely shared.

`amends` is **not** the instrument. That edge resolves to spec ids, and this
file is not a spec; an `amends` entry naming a file path does not resolve and is
a validation error rather than a governed constitutional change.

The anchor is the heading slug the indexer computes, so `## V. Legacy as
evidence` is `v-legacy-as-evidence`.

Unlike an amended `spec.md`, which records what the corpus held when it was
ratified, this document is a standing statement of what is true now. It is
edited in place, and its history lives in the specs that claimed each section
and in git.

**Say what the gate actually does in your repository, after measuring it.**
`standards/spec/constitution.md` is on the coupling gate's built-in bypass
floor, but an explicit, ownership-bearing unit claim overrides that floor. So an
edit here raises `C-001` if a spec claims this file as an explicit unit, and
raises nothing if none does. Both arrangements are legitimate; only one is true
of any given repository. Probe it against your pinned revision rather than
copying a sentence from another corpus, and then state the result plainly, so no
reader mistakes an unenforced claim for enforcement or the reverse.
