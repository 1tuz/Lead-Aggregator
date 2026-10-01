import directory from './data/russian-geography.json';

export type RussianCity = {
  name: string;
  region: string;
  autonomousDistrict: string | null;
};

export const russianCities = directory.cities as RussianCity[];
export const russianRegions = directory.regions as string[];

export function normalizePlaceName(value: string): string {
  return value.toLocaleLowerCase('ru').replaceAll('ё', 'е').trim();
}

export function citySlug(cityName: string): string {
  // ponytail: current SearchRequest carries one shared city slug; use a deterministic fallback until verified provider-specific slugs exist.
  const letters: Record<string, string> = {
    а: 'a', б: 'b', в: 'v', г: 'g', д: 'd', е: 'e', ё: 'e', ж: 'zh', з: 'z', и: 'i', й: 'y',
    к: 'k', л: 'l', м: 'm', н: 'n', о: 'o', п: 'p', р: 'r', с: 's', т: 't', у: 'u', ф: 'f',
    х: 'kh', ц: 'ts', ч: 'ch', ш: 'sh', щ: 'shch', ы: 'y', э: 'e', ю: 'yu', я: 'ya',
  };
  return [...cityName.toLocaleLowerCase('ru')]
    .map((letter) => letters[letter] ?? (letter === 'ь' || letter === 'ъ' ? '' : letter))
    .join('')
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .replace(/[^a-z0-9]+/g, '-')
    .replace(/^-|-$/g, '');
}

export function regionCityCount(region: string): number {
  return russianCities.filter((city) => city.region === region || city.autonomousDistrict === region).length;
}
