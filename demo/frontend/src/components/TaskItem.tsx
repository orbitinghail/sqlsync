import { Text, ActionIcon, Checkbox, Flex } from "@mantine/core";
import { useCallback } from "react";
import { Mutation } from "../doctype";
import { IconX } from "@tabler/icons-react";

export interface Task {
  id: string;
  description: string;
  completed: boolean;
}

export const TaskItem = ({
  task,
  mutate,
}: {
  task: Task;
  mutate: (m: Mutation) => Promise<void>;
}) => {
  const handleDelete = useCallback(() => {
    mutate({ tag: "DeleteTask", id: task.id }).catch((err) => {
      console.error("Failed to delete", err);
    });
  }, [task.id, mutate]);

  const handleToggleCompleted = useCallback(() => {
    mutate({ tag: "ToggleCompleted", id: task.id }).catch((err) => {
      console.error("Failed to toggle completed", err);
    });
  }, [task.id, mutate]);

  return (
    <Flex style={{ alignItems: "center" }} gap="sm">
      <Checkbox checked={task.completed} onChange={handleToggleCompleted} />
      <Text style={{ flex: 1 }}>{task.description}</Text>
      <ActionIcon color="red" variant="subtle" onClick={handleDelete}>
        <IconX />
      </ActionIcon>
    </Flex>
  );
};
