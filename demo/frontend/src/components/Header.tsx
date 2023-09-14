import {
  ActionIcon,
  Anchor,
  Center,
  Flex,
  Paper,
  Popover,
  Stack,
  Text,
  Title,
} from "@mantine/core";
import { IconQrcode } from "@tabler/icons-react";
import GitHubButton from "react-github-btn";
import QRCode from "react-qr-code";

const SQLSYNC_URL = "https://sqlsync.dev";

export const Header = () => {
  return (
    <>
      <Paper component={Stack} shadow="xs" p="xs" gap="sm">
        <Flex gap="sm">
          <Center component={Title} style={{ flex: 1, justifyContent: "left" }} order={4}>
            SQLSync Demo
          </Center>
          <GitHubButton
            href="https://github.com/orbitinghail/sqlsync"
            data-show-count="true"
            data-size="large"
            aria-label="Star orbitinghail/sqlsync on GitHub"
          >
            Star
          </GitHubButton>
          <Popover withArrow position="bottom">
            <Popover.Target>
              <ActionIcon>
                <IconQrcode />
              </ActionIcon>
            </Popover.Target>
            <Popover.Dropdown>
              <QRCode value={document.location.href} />
            </Popover.Dropdown>
          </Popover>
        </Flex>
        <Text>
          <Anchor href={SQLSYNC_URL}>SQLSync</Anchor> is a collaborative offline-first wrapper
          around SQLite. It is designed to synchronize web application state between users, devices,
          and the edge.
        </Text>
      </Paper>
    </>
  );
};
