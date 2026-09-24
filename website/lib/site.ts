// Site-wide links. Set these in your host's environment (e.g. Vercel project settings) before publishing;
// the placeholders make an unconfigured deploy obvious rather than silently broken.
export const LINKS = {
  download: process.env.NEXT_PUBLIC_DOWNLOAD_URL ?? "https://github.com/REPLACE-ME/openflow/releases/latest",
  source: process.env.NEXT_PUBLIC_SOURCE_URL ?? "https://github.com/REPLACE-ME/openflow",
};

export const SITE_URL = process.env.NEXT_PUBLIC_SITE_URL ?? "http://localhost:3000";
