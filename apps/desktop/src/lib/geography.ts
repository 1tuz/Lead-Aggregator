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

// Known catalog URL slugs (2GIS/Yell/Zoon). Blind transliteration breaks these.
const CITY_SLUG_OVERRIDES: Record<string, string> = {
  москва: 'moscow',
  'санкт-петербург': 'spb',
  казань: 'kazan',
  новосибирск: 'novosibirsk',
  екатеринбург: 'ekaterinburg',
  'нижний новгород': 'nizhny-novgorod',
  'ростов-на-дону': 'rostov',
  краснодар: 'krasnodar',
  самара: 'samara',
  уфа: 'ufa',
  красноярск: 'krasnoyarsk',
  воронеж: 'voronezh',
  пермь: 'perm',
  волгоград: 'volgograd',
  челябинск: 'chelyabinsk',
  омск: 'omsk',
};

export function citySlug(cityName: string): string {
  const normalized = normalizePlaceName(cityName);
  const override = CITY_SLUG_OVERRIDES[normalized];
  if (override) {
    return override;
  }

  // Shared UI slug. Providers remap where needed (Zoon: moscow -> msk).
  const letters: Record<string, string> = {
    а: 'a', б: 'b', в: 'v', г: 'g', д: 'd', е: 'e', ё: 'e', ж: 'zh', з: 'z', и: 'i', й: 'y',
    к: 'k', л: 'l', м: 'm', н: 'n', о: 'o', п: 'p', р: 'r', с: 's', т: 't', у: 'u', ф: 'f',
    х: 'kh', ц: 'ts', ч: 'ch', ш: 'sh', щ: 'shch', ы: 'y', э: 'e', ю: 'yu', я: 'ya',
  };
  return [...normalized]
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
