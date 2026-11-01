// The one list of questions the extension asks the person itself (`protocol/localApproval.ts`): `browser_type_text` asks here, `background/index.ts`
// shows them in the panel next to the hub's own approvals and relays the answers. A module of its own so the tool and the worker share the
// instance without importing each other.

import { LocalApprovals } from "../protocol/localApproval";

let notify: () => void = () => {};

export const localApprovals = new LocalApprovals(() => notify());

/** Sets what runs when the list of waiting questions changes (the worker tells the panel and redraws the icon). */
export function onLocalApprovalsChange(listener: () => void): void {
  notify = listener;
}
