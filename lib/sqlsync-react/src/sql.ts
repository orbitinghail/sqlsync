import { QueryKey, SqlValue } from "@orbitinghail/sqlsync-worker";
import { base58 } from "@scure/base";
import { sha256Digest } from "./util";

const UTF8_ENCODER = new TextEncoder();

export interface ParameterizedQuery {
  sql: string;
  params: SqlValue[];
}

export function normalizeQuery(query: ParameterizedQuery | string): ParameterizedQuery {
  if (typeof query === "string") {
    return { sql: query, params: [] };
  }
  return query;
}

/**
 * Returns a parameterized query object with the given SQL string and parameters.
 * This function should be used as a template literal tag.
 *
 * @example
 * const query = sql`SELECT * FROM users WHERE id = ${userId}`;
 *
 * @param chunks - An array of string literals.
 * @param params - An array of parameter values to be inserted into the SQL string.
 * @returns A parameterized query object with the given SQL string and parameters.
 */
export function sql(chunks: readonly string[], ...params: SqlValue[]): ParameterizedQuery {
  return {
    sql: chunks.join("?"),
    params,
  };
}

export async function toQueryKey(query: ParameterizedQuery): Promise<QueryKey> {
  const queryJson = JSON.stringify([query.sql, query.params]);
  const encoded = UTF8_ENCODER.encode(queryJson);
  const hashed = await sha256Digest(encoded);
  return base58.encode(new Uint8Array(hashed));
}
