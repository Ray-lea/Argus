import { Home, homeMetadata } from "@/views/Home";

export const metadata = homeMetadata("zh");

export default function Page() {
  return <Home lang="zh" />;
}
