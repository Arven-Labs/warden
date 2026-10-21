import { t, type Key } from "../i18n/index.ts";

/** P122 — the kinds of action an agent can be required to have approved even when it acts alone. The ids are those of
 * `warden_core::autonomy::Category`, as the config, the hub and `approvalRequired` write them. */
export interface ApprovalCategory {
  id: string;
  label: string;
}

const category = (id: string, key: Key): ApprovalCategory => ({
  id,
  get label() {
    return t(key);
  },
});

export const APPROVAL_CATEGORIES: ApprovalCategory[] = [
  category("delete_data", "approvalCategory.delete_data"),
  category("spend_money", "approvalCategory.spend_money"),
  category("critical_infra", "approvalCategory.critical_infra"),
  category("external_message", "approvalCategory.external_message"),
  category("publish_code", "approvalCategory.publish_code"),
  category("important_config", "approvalCategory.important_config"),
  category("elevated_agent", "approvalCategory.elevated_agent"),
];

/** A category's name, or the id itself when a newer hub sends one this screen does not know. */
export function approvalCategoryLabel(id: string): string {
  return APPROVAL_CATEGORIES.find((c) => c.id === id)?.label ?? id;
}
