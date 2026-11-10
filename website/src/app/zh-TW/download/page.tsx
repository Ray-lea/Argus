import { Download, downloadMetadata } from "@/views/Download";
import "@/styles/pages-route.css";

export const metadata = downloadMetadata("zh-TW");

export default function Page() {
  return <Download lang="zh-TW" />;
}
