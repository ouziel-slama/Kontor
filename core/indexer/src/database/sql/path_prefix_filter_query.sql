SELECT path
FROM (
  SELECT
    path,
    height,
    tx_id,
    ROW_NUMBER() OVER (PARTITION BY path ORDER BY height DESC, tx_id DESC) AS rank
  FROM
    contract_state
  WHERE
    contract_id = :contract_id
    AND path LIKE :path || '%'
    AND deleted = false
) t
WHERE
  rank = 1;