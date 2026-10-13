import type { ChatMessage, UsageSlice } from "../types";

/** What `send_message` reports about who answered a turn (P126). */
interface Reply {
  providerId?: string;
  /** The turn's usage told per agent and provider; only sent when more than one pair ran it. */
  usageSlices?: UsageSlice[];
}

/**
 * The `answeredBy` stamp the window saves with an answer: the provider that ran the turn's last call, and the usage told per
 * agent and provider when a reserve took over partway or sub-agents ran another model. Nothing when the turn says neither, so
 * the conversation's own selection applies. The backend adds the agent when it saves.
 */
export function answeredByOf(reply: Reply): { answeredBy: NonNullable<ChatMessage["answeredBy"]> } | Record<string, never> {
  const slices = reply.usageSlices && reply.usageSlices.length > 1 ? reply.usageSlices : undefined;
  if (!reply.providerId && !slices) return {};
  return {
    answeredBy: {
      ...(reply.providerId ? { providerId: reply.providerId } : {}),
      ...(slices ? { slices } : {}),
    },
  };
}
