import { Changelog, changelogMetadata } from "@/views/Changelog";
import "@/styles/pages-route.css";

export const metadata = changelogMetadata("en");

export default function Page() {
  return <Changelog lang="en" />;
}
