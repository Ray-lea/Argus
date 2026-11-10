import Replica from "@/components/replica/Replica";
import { Home, homeMetadata } from "@/views/Home";
import "@/styles/home-route.css";

export const metadata = homeMetadata("ru");

export default function Page() {
  return <Home lang="ru" demo={<Replica lang="ru" />} />;
}
