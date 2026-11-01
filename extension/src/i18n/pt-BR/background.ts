import type { background } from "../en/background.ts";

export const backgroundPt: Record<keyof typeof background, string> = {
  "bg.emptyMessage": "a mensagem está vazia",
  "bg.noHubId": "a mensagem ainda não tem id no hub",
  "bg.folderLocked": "a pasta é escolhida antes da primeira mensagem e não muda depois",
  "bg.typeDetailReplace": "Campo: {field}. Troca o conteúdo por ({length} caracteres): “{preview}”",
  "bg.typeDetailAppend": "Campo: {field}. Acrescenta ao conteúdo ({length} caracteres): “{preview}”",
};
