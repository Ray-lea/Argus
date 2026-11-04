import { DocsLayout } from "fumadocs-ui/layouts/spacious";
import { Download, House } from "lucide-react";
import type { ReactNode } from "react";
import { repo, site } from "@/lib/site";
import { source } from "@/lib/source";

// The layout of fumadocs.dev: the sidebar on the page background with the
// edition selector and search, the page on an inset panel beside it.
export default function Layout({ children }: { children: ReactNode }) {
  return (
    <DocsLayout
      tree={source.getPageTree()}
      nav={{
        title: (
          <>
            <img src="/assets/argus.svg" width={22} height={22} alt="" />
            Argus 文档
          </>
        ),
        url: "/",
      }}
      // Icon links only, beside the page's actions, as on fumadocs.dev; the title leads home.
      links={[
        { type: "icon", text: "官网首页", label: "官网首页", url: "/", icon: <House /> },
        { type: "icon", text: "下载", label: "下载 Argus " + site.release.tag, url: "/download/", icon: <Download /> },
        {
          type: "icon",
          text: "GitHub",
          label: "GitHub 仓库",
          url: repo,
          external: true,
          icon: (
            <svg role="img" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true">
              <path d="M12 .3a12 12 0 0 0-3.8 23.38c.6.12.83-.26.83-.57L9 21.07c-3.34.72-4.04-1.61-4.04-1.61-.55-1.39-1.34-1.76-1.34-1.76-1.08-.74.09-.73.09-.73 1.2.09 1.84 1.24 1.84 1.24 1.07 1.83 2.8 1.3 3.49 1 .1-.78.42-1.31.76-1.61-2.66-.3-5.47-1.33-5.47-5.93 0-1.31.47-2.38 1.24-3.22-.14-.3-.54-1.52.1-3.18 0 0 1-.32 3.3 1.23a11.5 11.5 0 0 1 6 0c2.28-1.55 3.29-1.23 3.29-1.23.64 1.66.24 2.88.12 3.18a4.65 4.65 0 0 1 1.23 3.22c0 4.61-2.8 5.63-5.48 5.92.42.36.81 1.1.81 2.22l-.01 3.29c0 .31.2.69.82.57A12 12 0 0 0 12 .3" />
            </svg>
          ),
        },
      ]}
      sidebar={{ defaultOpenLevel: 0 }}
      themeSwitch={{ mode: "light-dark-system" }}
    >
      {children}
    </DocsLayout>
  );
}
