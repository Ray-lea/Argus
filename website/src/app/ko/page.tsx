import Replica from "@/components/replica/Replica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("ko");

export default function Page() {
  return <Home lang="ko" demo={<Replica lang="ko" />} />;
}
