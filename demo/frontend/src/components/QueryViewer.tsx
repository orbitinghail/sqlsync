import { CodeHighlight } from "@mantine/code-highlight";
import { Alert, Button, Code, Collapse, Paper, Textarea } from "@mantine/core";
import { useDisclosure } from "@mantine/hooks";
import { JournalId } from "@orbitinghail/sqlsync-worker";
import { IconAlertCircle, IconCaretDownFilled, IconCaretRightFilled } from "@tabler/icons-react";
import { useMemo, useState } from "react";
import { useQuery } from "../doctype";

interface Props {
  docId: JournalId;
}

export const QueryViewerInner = ({ docId }: Props) => {
  const [inputValue, setInputValue] = useState("select * from tasks");
  const result = useQuery(docId, inputValue);

  const rowsJson = useMemo(() => {
    return JSON.stringify(
      result.rows ?? [],
      (_, value) => {
        // handle bigint values
        if (typeof value === "bigint") {
          return value.toString();
        }
        // eslint-disable-next-line @typescript-eslint/no-unsafe-return
        return value;
      },
      2
    );
  }, [result.rows]);

  let output;
  if (result.state === "error") {
    output = (
      <Alert color="red" variant="light" title="SQL Error" icon={<IconAlertCircle />} p="sm">
        <Code color="transparent">{result.error.message}</Code>
      </Alert>
    );
  } else {
    output = <CodeHighlight code={rowsJson} language="json" withCopyButton={false} />;
  }

  return (
    <>
      <Textarea
        mb="sm"
        autosize
        description="Run any SQL query. Available tables: tasks"
        value={inputValue}
        styles={{ input: { fontFamily: "monospace" } }}
        onChange={(e) => setInputValue(e.currentTarget.value)}
      />
      {output}
    </>
  );
};

export const QueryViewer = (props: Props) => {
  const [visible, { toggle }] = useDisclosure();
  const icon = visible ? <IconCaretDownFilled /> : <IconCaretRightFilled />;

  return (
    <Paper>
      <Button
        variant="subtle"
        fullWidth
        leftSection={icon}
        size="compact-md"
        styles={{ inner: { justifyContent: "left" } }}
        onClick={toggle}
        mb="sm"
      >
        Query Viewer
      </Button>
      <Collapse in={visible}>
        <QueryViewerInner {...props} />
      </Collapse>
    </Paper>
  );
};
