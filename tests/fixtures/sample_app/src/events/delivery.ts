// Delivery signs webhook events and sends them over HTTPS, with a safe dry-run path.
import { createHmac } from 'node:crypto';
import type { DeliveryReceipt } from '../types.ts';
import { FunctionError } from '../utils/errors.ts';

export async function dispatchWebhookEvent(params: {
  endpointUrl: string;
  eventType: string;
  maxRetries?: number;
  dryRun?: boolean;
}): Promise<DeliveryReceipt> {
  // 1. Reject any endpoint that does not use HTTPS.
  if (!params.endpointUrl.startsWith('https://')) {
    throw new FunctionError('INSECURE_PROTOCOL', { endpointUrl: params.endpointUrl });
  }

  // 2. Read the signing secret from the process environment.
  const secret = process.env.WEBHOOK_SIGNING_SECRET ?? '';

  // 3. Sign the timestamp and event topic with HMAC-SHA256.
  const timestamp = Math.floor(Date.now() / 1000);
  const signature = createHmac('sha256', secret)
    .update(`${timestamp}.${params.eventType}`)
    .digest('hex');

  // 4. Short-circuit delivery when dryRun is enabled.
  if (params.dryRun) {
    return {
      deliveryId: `dlv_${timestamp.toString(16)}${Math.random().toString(16).slice(2, 8)}`,
      signatureHeader: `t=${timestamp},v1=${signature}`,
      attemptsQueued: params.maxRetries ?? 3,
      status: 'simulated'
    };
  }

  // 5. Deliver the signed payload over the network.
  const response = await fetch(params.endpointUrl, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'X-Fnscope-Signature': `t=${timestamp},v1=${signature}`
    },
    body: JSON.stringify({ eventType: params.eventType, timestamp })
  });

  // 6. Surface a failed delivery as a typed error.
  if (!response.ok) {
    throw new FunctionError('DELIVERY_FAILED', { status: response.status });
  }

  // 7. Return a receipt describing the delivered attempt.
  return {
    deliveryId: `dlv_${timestamp.toString(16)}${Math.random().toString(16).slice(2, 8)}`,
    signatureHeader: `t=${timestamp},v1=${signature}`,
    attemptsQueued: params.maxRetries ?? 3,
    status: 'delivered'
  };
}
