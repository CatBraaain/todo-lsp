const MINIMUM_NODE_MAJOR = 20;

export function assertNode20OrLater(nodeVersion) {
  const [majorText] = nodeVersion.split(".", 1);
  const major = Number(majorText);
  if (Number.isInteger(major) && major >= MINIMUM_NODE_MAJOR) return;

  throw new Error(
    `Todo requires Node.js ${MINIMUM_NODE_MAJOR} or later. VS Code is running Node.js ${nodeVersion}. Update VS Code to a version that includes Node.js ${MINIMUM_NODE_MAJOR} or later.`,
  );
}
