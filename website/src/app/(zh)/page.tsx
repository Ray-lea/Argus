import Replica from "@/components/replica/Replica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("zh");

export default function Page() {
  return <Home lang="zh" demo={<Replica />} />;
}
