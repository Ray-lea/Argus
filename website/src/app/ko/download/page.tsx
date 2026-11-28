import { Download, downloadMetadata } from "@/views/Download";
import "@/styles/pages-route.css";

export const metadata = downloadMetadata("ko");

export default function Page() {
  return <Download lang="ko" />;
}
