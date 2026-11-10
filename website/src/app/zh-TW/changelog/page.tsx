import { Changelog, changelogMetadata } from "@/views/Changelog";
import "@/styles/pages-route.css";

export const metadata = changelogMetadata("zh-TW");

export default function Page() {
  return <Changelog lang="zh-TW" />;
}
