import { Changelog, changelogMetadata } from "@/views/Changelog";

export const metadata = changelogMetadata("en");

export default function Page() {
  return <Changelog lang="en" />;
}
