import { Container, Stack } from "@mantine/core";
import { JournalId } from "@orbitinghail/sqlsync-worker";
import { useEffect } from "react";
import { Header } from "./components/Header";
import { QueryViewer } from "./components/QueryViewer";
import { TaskList } from "./components/TaskList";
import { useMutate } from "./doctype";

export const App = ({ docId }: { docId: JournalId }) => {
  const mutate = useMutate(docId);

  useEffect(() => {
    mutate({ tag: "InitSchema" }).catch((err) => {
      console.error("Failed to init schema", err);
    });
  }, [mutate]);

  return (
    <Container size="xs" py="sm">
      <Stack>
        <Header />
        <TaskList docId={docId} />
        <QueryViewer docId={docId} />
      </Stack>
    </Container>
  );
};
