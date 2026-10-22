/** @type {import("next").NextConfig} */
const config = {
  // Static export: the docs are plain HTML, no server needed. Routes that need a
  // server (the account side of the site) come later and will drop this.
  output: "export",
  trailingSlash: true,
};

export default config;
