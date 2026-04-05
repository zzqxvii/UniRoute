interface SkeletonProps {
  className?: string;
  width?: string | number;
  height?: string | number;
}

export function Skeleton({ className = '', width, height }: SkeletonProps) {
  return (
    <div
      className={`animate-pulse bg-gray-200 dark:bg-gray-700 rounded ${className}`}
      style={{ width, height }}
    />
  );
}

export function DashboardSkeleton() {
  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <Skeleton className="h-8 w-32" />
        <Skeleton className="h-6 w-16" />
      </div>

      <div className="grid grid-cols-1 gap-4 sm:grid-cols-4">
        {[...Array(4)].map((_, i) => (
          <div key={i} className="bg-white dark:bg-gray-800 rounded-lg shadow p-5">
            <Skeleton className="h-4 w-20 mb-2" />
            <Skeleton className="h-6 w-24" />
          </div>
        ))}
      </div>

      <div className="mt-6">
        <Skeleton className="h-6 w-24 mb-4" />
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow overflow-hidden">
          <div className="bg-gray-50 dark:bg-gray-900/50 p-4">
            <div className="flex gap-4">
              <Skeleton className="h-4 w-16" />
              <Skeleton className="h-4 w-48" />
              <Skeleton className="h-4 w-32" />
            </div>
          </div>
          {[...Array(4)].map((_, i) => (
            <div key={i} className="p-4 border-t border-gray-200 dark:border-gray-700">
              <div className="flex gap-4 items-center">
                <Skeleton className="h-6 w-14 rounded-full" />
                <Skeleton className="h-5 w-48" />
                <Skeleton className="h-5 w-32" />
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export function ProvidersSkeleton() {
  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <Skeleton className="h-8 w-40" />
        <Skeleton className="h-10 w-32" />
      </div>
      <div className="space-y-4">
        {[...Array(3)].map((_, i) => (
          <div key={i} className="bg-white dark:bg-gray-800 rounded-lg shadow p-5">
            <div className="flex items-center gap-4">
              <Skeleton className="h-10 w-10 rounded-full" />
              <div className="flex-1">
                <Skeleton className="h-5 w-32 mb-2" />
                <Skeleton className="h-4 w-48" />
              </div>
              <Skeleton className="h-8 w-8 rounded" />
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export function GroupsSkeleton() {
  return (
    <div className="px-4 py-6 sm:px-0">
      <div className="flex justify-between items-center mb-6">
        <Skeleton className="h-8 w-32" />
        <Skeleton className="h-10 w-28" />
      </div>
      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {[...Array(4)].map((_, i) => (
          <div key={i} className="bg-white dark:bg-gray-800 rounded-lg shadow p-5">
            <Skeleton className="h-5 w-24 mb-3" />
            <Skeleton className="h-4 w-full mb-2" />
            <Skeleton className="h-4 w-3/4" />
          </div>
        ))}
      </div>
    </div>
  );
}

export function SettingsSkeleton() {
  return (
    <div className="px-4 py-6 sm:px-0">
      <Skeleton className="h-8 w-20 mb-6" />
      <div className="bg-white dark:bg-gray-800 rounded-lg shadow p-6 max-w-2xl">
        {[...Array(5)].map((_, i) => (
          <div key={i} className="mb-6">
            <Skeleton className="h-4 w-24 mb-2" />
            <Skeleton className="h-10 w-full" />
          </div>
        ))}
        <Skeleton className="h-10 w-24" />
      </div>
    </div>
  );
}
