/** @type {import("next").NextConfig} */
const config = {
  // Static export: the docs are plain HTML, no server needed. Routes that need a
  // server (the account side of the site) come later and will drop this.
  output: "export",
  trailingSlash: true,
  // Set only by the Pages workflow (the site lives under /warden there); empty on a
  // custom domain and in local dev.
  basePath: process.env.SITE_BASE_PATH ?? "",
};

export default config;
