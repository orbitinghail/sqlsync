import { rem, Button } from "@mantine/core";
import { useConnectionStatus } from "@orbitinghail/sqlsync-react";
import { JournalId } from "@orbitinghail/sqlsync-worker";
import { useCallback } from "react";
import { useSetConnectionEnabled } from "../doctype";
import { IconWifi, IconWifiOff } from "@tabler/icons-react";

export const ConnectionStatus = ({ docId }: { docId: JournalId }) => {
  const status = useConnectionStatus();
  const setConnectionEnabled = useSetConnectionEnabled(docId);

  const handleClick = useCallback(() => {
    if (status === "disabled") {
      setConnectionEnabled(true).catch((err) => {
        console.error("Failed to enable connection", err);
      });
    } else {
      setConnectionEnabled(false).catch((err) => {
        console.error("Failed to disable connection", err);
      });
    }
  }, [status, setConnectionEnabled]);

  let color, icon, loading;
  switch (status) {
    case "disabled":
      color = "gray";
      icon = <IconWifiOff style={{ width: rem(16), height: rem(16) }} />;
      break;
    case "disconnected":
      color = "gray";
      icon = <IconWifiOff style={{ width: rem(16), height: rem(16) }} />;
      break;
    case "connecting":
      color = "yellow";
      loading = true;
      break;
    case "connected":
      color = "green";
      icon = <IconWifi style={{ width: rem(16), height: rem(16) }} />;
      break;
  }

  return (
    <Button
      variant="light"
      color={color}
      rightSection={icon}
      loading={loading}
      onClick={handleClick}
      size="compact-md"
    >
      {status}
    </Button>
  );
};
