// The fixture match does not exercise automatic team assignment.
export default function init(): never {
  throw new Error('Automatic team assignment is unavailable in the fixture match.')
}

export const autoAssign = init
