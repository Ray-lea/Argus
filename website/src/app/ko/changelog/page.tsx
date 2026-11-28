import { Changelog, changelogMetadata } from "@/views/Changelog";
import "@/styles/pages-route.css";

export const metadata = changelogMetadata("ko");

export default function Page() {
  return <Changelog lang="ko" />;
}
