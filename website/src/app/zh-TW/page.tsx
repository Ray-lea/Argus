import Replica from "@/components/replica/Replica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("zh-TW");

export default function Page() {
  return <Home lang="zh-TW" demo={<Replica lang="zh-TW" />} />;
}
