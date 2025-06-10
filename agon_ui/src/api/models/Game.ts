/* generated using openapi-typescript-codegen -- do not edit */
/* istanbul ignore file */
/* tslint:disable */
/* eslint-disable */
import type { GameType } from './GameType';
import type { Location } from './Location';
export type Game = {
    id: string;
    title: string;
    game_type: GameType;
    location: Location;
    scheduled_time: string;
    duration_minutes: number;
    created_by_user_id: string;
    created_at: string;
    status: string;
};

