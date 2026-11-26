// Shared shapes keep every module free of runtime type-only imports.

// A word pulled from a sentence, preserving both spelling and lowercase form.
export interface Token {
  index: number;
  raw: string;
  normalized: string;
}

// A plain-English grammatical job assigned to a matched token.
export interface SyntacticJob {
  id: 'doer' | 'engine' | 'receiver' | 'bridge' | 'painter';
  label: string;
  confidence: number;
  intuition: string;
}

// Payload returned after classifying a target word in context.
export interface RoleEvaluationResult {
  roleId: 'doer' | 'engine' | 'receiver' | 'bridge' | 'painter';
  label: string;
  confidence: number;
  intuition?: string;
}

// Verdict produced when a learner answer is graded.
export interface GradeOutcome {
  correct: boolean;
  expectedRole: RoleEvaluationResult;
  receipt: DeliveryReceipt;
}

// Metadata describing one webhook delivery attempt.
export interface DeliveryReceipt {
  deliveryId: string;
  signatureHeader: string;
  attemptsQueued: number;
  status: 'simulated' | 'delivered';
}
