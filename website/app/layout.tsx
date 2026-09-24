import type { Metadata, Viewport } from "next";
import { Archivo } from "next/font/google";
import { SITE_URL } from "@/lib/site";
import "./globals.css";

// One engineered grotesk, stretched wide for headlines and narrowed for key legends (see DESIGN.md).
const archivo = Archivo({
  subsets: ["latin"],
  axes: ["wdth"],
  variable: "--font-archivo",
});

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: "OpenFlow — hold ⌘; and talk. Private dictation for your Mac.",
  description:
    "OpenFlow turns what you say into clean text wherever your cursor is. Speech recognition and cleanup run on your Mac. Meeting notes without a bot. Open source.",
  openGraph: {
    title: "OpenFlow — hold ⌘; and talk",
    description: "Private, local-first dictation and meeting notes for the Mac. Open source.",
    images: ["/assets/app-home.jpg"],
  },
};

export const viewport: Viewport = {
  themeColor: "#1d3fd6",
};

export default function RootLayout({ children }: LayoutProps<"/">) {
  return (
    <html lang="en" className={archivo.variable}>
      <body>{children}</body>
    </html>
  );
}
