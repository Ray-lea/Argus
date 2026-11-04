import { Changelog, changelogMetadata } from "@/views/Changelog";

export const metadata = changelogMetadata("zh");

export default function Page() {
  return <Changelog lang="zh" />;
}
