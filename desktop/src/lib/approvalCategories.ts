import { t, type Key } from "../i18n/index.ts";

/** P122 — the kinds of action an agent can be told to get approved even when it otherwise acts alone. The ids are
 * `warden_core::autonomy::Category`'s, as the config file, the hub and `approvalRequired` write them. */
export interface ApprovalCategory {
  id: string;
  label: string;
  hint: string;
}

// Getters, so each read says the words of the language the person has now.
const category = (id: string): ApprovalCategory => ({
  id,
  get label() {
    return t(`approvalCategory.${id}` as Key);
  },
  get hint() {
    return t(`approvalCategory.${id}.hint` as Key);
  },
});

export const APPROVAL_CATEGORIES: ApprovalCategory[] = [
  category("delete_data"),
  category("spend_money"),
  category("critical_infra"),
  category("external_message"),
  category("publish_code"),
  category("important_config"),
  category("elevated_agent"),
];

/** The label of a category id, or the id itself when a newer hub sends one this app doesn't know. */
export function approvalCategoryLabel(id: string): string {
  return APPROVAL_CATEGORIES.find((c) => c.id === id)?.label ?? id;
}
