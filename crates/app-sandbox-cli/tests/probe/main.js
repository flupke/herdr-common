// The smallest mod: what the test installs in a sandbox and opens in a tab.
import { View, div } from "gpui-kit";

export default class Probe extends View {
  render() {
    return div().child("probe");
  }
}
