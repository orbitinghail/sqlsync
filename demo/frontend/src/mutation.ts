// matches the Mutation type in demo/demo-reducer
export type Mutation =
  | {
      tag: "InitSchema";
    }
  | {
      tag: "CreateTask";
      id: string;
      description: string;
    }
  | {
      tag: "DeleteTask";
      id: string;
    }
  | {
      tag: "ToggleCompleted";
      id: string;
    };
