import { Download, downloadMetadata } from "@/views/Download";

export const metadata = downloadMetadata("en");

export default function Page() {
  return <Download lang="en" />;
}
