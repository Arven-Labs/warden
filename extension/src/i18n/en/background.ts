// Errors the background worker sends back to the panel.
export const background = {
  "bg.emptyMessage": "the message is empty",
  "bg.noHubId": "the message has no id on the hub yet",
  "bg.folderLocked": "the folder is chosen before the first message and does not change afterwards",
  "bg.typeDetailReplace": "Field: {field}. Replaces its content with ({length} characters): “{preview}”",
  "bg.typeDetailAppend": "Field: {field}. Adds to its content ({length} characters): “{preview}”",
} as const;
