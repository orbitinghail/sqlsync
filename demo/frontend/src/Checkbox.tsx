// with thanks to https://marek-rozmus.medium.com/styling-checkbox-with-tailwind-46a92c157e2d

const Checkbox = (
  props: React.DetailedHTMLProps<
    React.InputHTMLAttributes<HTMLInputElement>,
    HTMLInputElement
  >
) => (
  <div className="w-full flex gap-2">
    <input
      className="
        peer relative appearance-none shrink-0 w-4 h-4 border-2 border-blue-200 rounded-sm mt-1 bg-white
        focus:outline-none focus:ring-offset-0 focus:ring-1 focus:ring-blue-100
        checked:bg-blue-500 checked:border-0
        disabled:border-steel-400 disabled:bg-steel-400
      "
      type="checkbox"
      {...props}
    />
    <svg
      className="absolute w-4 h-4 pointer-events-none hidden peer-checked:block stroke-white mt-1 outline-none"
      xmlns="http://www.w3.org/2000/svg"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="4"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      <polyline points="20 6 9 17 4 12"></polyline>
    </svg>
  </div>
);

export default Checkbox;
