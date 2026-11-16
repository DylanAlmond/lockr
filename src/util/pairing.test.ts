import { describe, expect, it } from 'vitest';
import { joinGate, pairingHandoffText, typedJoinComplete } from './pairing';

describe('joinGate', () => {
  it('allows a Device with no User to join', () => {
    const gate = joinGate([]);

    expect(gate.canJoin).toBe(true);
    expect(gate.blockedReason).toBeNull();
  });

  it('names the User a Device already belongs to', () => {
    const gate = joinGate([{ name: 'Alice' }]);

    expect(gate.canJoin).toBe(false);
    expect(gate.blockedReason).toContain('already belongs to Alice');
    expect(gate.blockedReason).toContain('cannot join');
  });

  it('stays blocked for every registered User', () => {
    const gate = joinGate([{ name: 'Alice' }, { name: 'Bob' }]);

    expect(gate.canJoin).toBe(false);
    expect(gate.blockedReason).not.toBeNull();
  });
});

describe('pairingHandoffText', () => {
  it('puts everything the typed path needs in one block', () => {
    const text = pairingHandoffText({
      typed_code: 'ABCD-FGHJ',
      source_device_id: '4f0a2c1e-7d3b-4a6e-9c2f-0b7d5e8a1c33'
    });

    expect(text).toBe(
      'Pairing code: ABCD-FGHJ\nDevice id: 4f0a2c1e-7d3b-4a6e-9c2f-0b7d5e8a1c33'
    );
  });
});

describe('typedJoinComplete', () => {
  it('needs both the source Device id and the code', () => {
    expect(typedJoinComplete('', '')).toBe(false);
    expect(typedJoinComplete('4f0a2c1e', '')).toBe(false);
    expect(typedJoinComplete('', 'ABCD-FGHJ')).toBe(false);
    expect(typedJoinComplete('4f0a2c1e', 'ABCD-FGHJ')).toBe(true);
  });

  it('ignores whitespace around pasted values', () => {
    expect(typedJoinComplete('  4f0a2c1e  ', '  ABCD-FGHJ ')).toBe(true);
  });
});
