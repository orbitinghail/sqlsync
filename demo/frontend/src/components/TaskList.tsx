import { Center, Flex, Paper, Stack, Title } from "@mantine/core";
import { sql } from "@orbitinghail/sqlsync-react";
import { JournalId } from "@orbitinghail/sqlsync-worker";
import { ConnectionStatus } from "./ConnectionStatus";
import { useMutate, useQuery } from "../doctype";
import { Task, TaskItem } from "./TaskItem";
import { TaskForm } from "./TaskForm";

export const TaskList = ({ docId }: { docId: JournalId }) => {
  const { rows: tasks } = useQuery<Task>(
    docId,
    sql`select id, description, completed from tasks order by description`
  );
  const mutate = useMutate(docId);

  return (
    <Paper component={Stack} shadow="xs" p="xs">
      <Flex>
        <Center component={Title} style={{ flex: 1, justifyContent: "left" }} order={5}>
          Tasks
        </Center>
        <ConnectionStatus docId={docId} />
      </Flex>
      {(tasks ?? []).map((task) => (
        <TaskItem key={task.id} task={task} mutate={mutate} />
      ))}
      <TaskForm mutate={mutate} />
    </Paper>
  );
};
