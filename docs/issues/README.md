# Repository Issues

HomeMagic tracks implementation work as versioned Markdown so decisions,
acceptance criteria, status, and evidence evolve with the code.

## Frontmatter

Every issue uses these fields:

- `id`: stable project-local identifier;
- `epic`: owning epic;
- `title`: short English outcome;
- `status`: `planned`, `ready`, `in_progress`, `blocked`, or `done`;
- `priority`: `critical`, `high`, `medium`, or `low`;
- `depends_on`: issue IDs that must be complete first;
- `adrs`: related decision records;
- `created` and `updated`: ISO dates.

## Completion rules

An issue becomes `done` only after every acceptance criterion and verification
item has linked evidence. Implementation and issue status should normally change
in the same commit. Partial work remains unchecked and is described in the
progress log.

## Active epic

- [EPIC-001 issue index](epic-001/README.md)
- [EPIC-002 issue index](epic-002/README.md)
- [EPIC-003 issue index](epic-003/README.md)
- [EPIC-004 issue index](epic-004/README.md)
