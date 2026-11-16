import { describe, expect, it } from 'vitest';
import {
  HIDDEN_SECRET,
  clearReveals,
  conflictSideKey,
  displayConflictSideValue,
  formatConflictValue,
  hideSide,
  isSideRevealed,
  revealSide,
  type RevealedConflictValues
} from './conflictRedaction';

const SECRET = 'hunter2-correct-horse';

const local = { device: 'device-a', counter: 3 };
const remote = { device: 'device-b', counter: 7 };

describe('secret redaction', () => {
  it('never renders a secret that reached the report without an explicit reveal', () => {
    // The backend withholds secret values from Conflicts. This pins the second
    // line of defence: if one ever arrives by mistake, it still cannot render.
    const rendered = displayConflictSideValue({
      isSecret: true,
      reportedValue: SECRET,
      revealed: false
    });

    expect(rendered).toBe(HIDDEN_SECRET);
    expect(rendered).not.toContain(SECRET);
  });

  it('renders an untouched secret as hidden even when its reveal map is empty', () => {
    expect(
      displayConflictSideValue({
        isSecret: true,
        reportedValue: null,
        revealed: false
      })
    ).toBe(HIDDEN_SECRET);
  });

  it('renders exactly the explicitly revealed value', () => {
    expect(
      displayConflictSideValue({
        isSecret: true,
        reportedValue: null,
        revealed: true,
        revealedValue: SECRET
      })
    ).toBe(SECRET);
  });

  it('distinguishes an unrevealed secret from a revealed but unset one', () => {
    expect(
      displayConflictSideValue({
        isSecret: true,
        reportedValue: null,
        revealed: true,
        revealedValue: null
      })
    ).toBe('Not set');
  });

  it('ignores the reported value once a secret is revealed', () => {
    expect(
      displayConflictSideValue({
        isSecret: true,
        reportedValue: 'stale-report-value',
        revealed: true,
        revealedValue: SECRET
      })
    ).toBe(SECRET);
  });
});

describe('non-secret conflict values', () => {
  it.each([
    [null, 'Not set'],
    [undefined, 'Not set'],
    ['', 'Empty'],
    ['octocat', 'octocat'],
    [true, 'Yes'],
    [false, 'No'],
    [42, '42'],
    [[], 'None'],
    [['work', 'personal'], 'work, personal'],
    [{ a: 1 }, '{"a":1}']
  ])('formats %j as %s', (value, expected) => {
    expect(formatConflictValue(value)).toBe(expected);
    expect(
      displayConflictSideValue({ isSecret: false, reportedValue: value, revealed: false })
    ).toBe(expected);
  });
});

describe('reveal and hide transitions', () => {
  it('keys a reveal by the writer identity', () => {
    expect(conflictSideKey(local)).toBe('device-a:3');
    expect(conflictSideKey(remote)).toBe('device-b:7');
  });

  it('reveals one side without revealing the other', () => {
    const revealed = revealSide({}, local, SECRET);

    expect(isSideRevealed(revealed, local)).toBe(true);
    expect(isSideRevealed(revealed, remote)).toBe(false);
  });

  it('hides a revealed side and leaves the others intact', () => {
    const both = revealSide(revealSide({}, local, 'first'), remote, 'second');

    const hidden = hideSide(both, local);
    expect(isSideRevealed(hidden, local)).toBe(false);
    expect(hidden[conflictSideKey(remote)]).toBe('second');
  });

  it('ignore hiding a side that was never revealed', () => {
    const revealed: RevealedConflictValues = revealSide({}, remote, SECRET);

    expect(hideSide(revealed, local)).toBe(revealed);
  });

  it('re-mask every side when the Conflict changes', () => {
    const both = revealSide(revealSide({}, local, 'first'), remote, 'second');

    const cleared = clearReveals();
    expect(isSideRevealed(cleared, local)).toBe(false);
    expect(isSideRevealed(cleared, remote)).toBe(false);
    expect(both).not.toBe(cleared);
  });

  it('treats an explicit null reveal as revealed, not as absent', () => {
    const revealed = revealSide({}, local, null);

    expect(isSideRevealed(revealed, local)).toBe(true);
    expect(Object.prototype.hasOwnProperty.call(revealed, conflictSideKey(local))).toBe(true);
  });

  it('does not mutate the previous reveal map', () => {
    const before: RevealedConflictValues = {};
    const after = revealSide(before, local, SECRET);

    expect(before).toEqual({});
    expect(after).not.toBe(before);
  });
});
