import { Download, downloadMetadata } from "@/views/Download";

export const metadata = downloadMetadata("zh");

export default function Page() {
  return <Download lang="zh" />;
}
