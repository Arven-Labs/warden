import type { ChatMessage, UsageSlice } from "../types";

/** What `send_message` reports about who answered a turn (P126). */
interface Reply {
  providerId?: string;
  /** The combo (P90) the turn's model was picked as, when it was one. */
  comboId?: string;
  /** The turn's usage told per agent and provider; only sent when more than one pair ran it. */
  usageSlices?: UsageSlice[];
}

/**
 * The `answeredBy` stamp the window saves with an answer: the provider that ran the turn's last call, the combo the model was
 * picked as, and the usage told per agent and provider when a reserve took over partway or sub-agents ran another model. Nothing
 * when the turn says none of them, so the conversation's own selection applies. The backend adds the agent when it saves.
 */
export function answeredByOf(reply: Reply): { answeredBy: NonNullable<ChatMessage["answeredBy"]> } | Record<string, never> {
  const slices = reply.usageSlices && reply.usageSlices.length > 1 ? reply.usageSlices : undefined;
  if (!reply.providerId && !reply.comboId && !slices) return {};
  return {
    answeredBy: {
      ...(reply.providerId ? { providerId: reply.providerId } : {}),
      ...(reply.comboId ? { comboId: reply.comboId } : {}),
      ...(slices ? { slices } : {}),
    },
  };
}
