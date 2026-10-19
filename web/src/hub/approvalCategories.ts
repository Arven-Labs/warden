import { t, type Key } from "../i18n/index.ts";

/** P122 — os tipos de ação que um agente pode ser obrigado a ter aprovados mesmo quando age sozinho. Os ids são os de
 * `warden_core::autonomy::Category`, como o config, o hub e `approvalRequired` os escrevem. */
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

/** O nome de uma categoria, ou o próprio id quando um hub mais novo manda uma que esta tela não conhece. */
export function approvalCategoryLabel(id: string): string {
  return APPROVAL_CATEGORIES.find((c) => c.id === id)?.label ?? id;
}
